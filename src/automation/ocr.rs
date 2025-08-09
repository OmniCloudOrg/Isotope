use anyhow::{Context, Result};
use image::DynamicImage;
use tracing::{debug, info, trace, warn};
use ocrs::{OcrEngine as OcrsEngine, OcrEngineParams, ImageSource, DecodeMethod, DimOrder};
use rten_tensor::AsView;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ring::digest;
use tokio::sync::{mpsc, oneshot, broadcast};
use async_trait::async_trait;

use super::models::{load_model, ModelSource};

/// Screen change event notification
#[derive(Debug, Clone)]
pub struct ScreenChangeEvent {
    pub old_state: Option<ScreenState>,
    pub new_state: ScreenState,
    pub timestamp: Instant,
}

/// Background monitoring commands
#[derive(Debug)]
pub enum MonitorCommand {
    Pause,
    Resume,
    UpdateInterval(Duration),
    Shutdown,
    GetStatus(oneshot::Sender<MonitorStatus>),
}

/// Status of the background monitor
#[derive(Debug, Clone)]
pub struct MonitorStatus {
    pub is_running: bool,
    pub is_paused: bool,
    pub current_interval: Duration,
    pub last_update: Option<Instant>,
    pub total_updates: u64,
    pub current_state: Option<ScreenState>,
}

/// Screenshot capture trait for pluggable screenshot backends
#[async_trait]
pub trait ScreenshotCapture: Send + Sync {
    async fn capture(&self) -> Result<DynamicImage>;
}

/// Represents the current state of the screen as detected by OCR
#[derive(Debug, Clone)]
pub struct ScreenState {
    /// Extracted text content from the screen
    pub text: String,
    /// Hash of the raw image data for change detection
    pub image_hash: String,
    /// Timestamp when this state was captured
    pub timestamp: Instant,
    /// Percentage of black pixels (useful for detecting boot screens, etc.)
    pub black_percentage: usize,
    /// Percentage of white pixels
    pub white_percentage: usize,
    /// Screen dimensions
    pub dimensions: (u32, u32),
}

impl ScreenState {
    /// Check if this screen state is stale (older than threshold)
    pub fn is_stale(&self, threshold: Duration) -> bool {
        self.timestamp.elapsed() > threshold
    }
    
    /// Check if this represents a likely boot/BIOS screen
    pub fn is_boot_screen(&self) -> bool {
        self.black_percentage > 90 && self.text.trim().is_empty()
    }
    
    /// Check if this represents a blank/minimal screen
    pub fn is_minimal_screen(&self) -> bool {
        (self.black_percentage > 95 || self.white_percentage > 95) && self.text.len() < 10
    }
}

pub struct OcrEngine {
    engine: OcrsEngine,
    /// Cached screen state to avoid race conditions
    screen_state: Arc<RwLock<Option<ScreenState>>>,
    /// Minimum time between screen updates to avoid excessive OCR processing
    update_threshold: Duration,
    /// Command channel for controlling background monitoring
    monitor_tx: Option<mpsc::UnboundedSender<MonitorCommand>>,
    /// Receiver for screen change events
    change_rx: broadcast::Receiver<ScreenChangeEvent>,
    /// Sender for screen change events (kept for cloning)
    change_tx: broadcast::Sender<ScreenChangeEvent>,
}

/// Default text detection model.
const DETECTION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";

/// Default text recognition model.
const RECOGNITION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

impl OcrEngine {
    pub fn new() -> Self {
        Self::with_options(false, Duration::from_millis(500))
    }
    
    pub fn with_beam_search() -> Self {
        Self::with_options(true, Duration::from_millis(500))
    }
    
    pub fn with_update_threshold(threshold: Duration) -> Self {
        Self::with_options(false, threshold)
    }
    
    fn with_options(beam_search: bool, update_threshold: Duration) -> Self {
        info!("Initializing enhanced OCR engine using ocrs with pre-trained models");
        
        // Load detection model
        info!("Loading text detection model...");
        let detection_model_src = ModelSource::Url(DETECTION_MODEL.to_string());
        let detection_model = load_model(detection_model_src)
            .expect("Failed to load text detection model");
        
        // Load recognition model
        info!("Loading text recognition model...");
        let recognition_model_src = ModelSource::Url(RECOGNITION_MODEL.to_string());
        let recognition_model = load_model(recognition_model_src)
            .expect("Failed to load text recognition model");
        
        // Create OCR engine with enhanced parameters
        let decode_method = if beam_search {
            DecodeMethod::BeamSearch { width: 100 }
        } else {
            DecodeMethod::Greedy
        };
        
        let engine_params = OcrEngineParams {
            detection_model: Some(detection_model),
            recognition_model: Some(recognition_model),
            decode_method,
            debug: false,
            alphabet: None,
            allowed_chars: None,
            ..Default::default()
        };
        
        let engine = OcrsEngine::new(engine_params)
            .expect("Failed to initialize OCR engine");
        
        info!("OCR engine initialized with enhanced models and {} decoding", 
              if beam_search { "beam search" } else { "greedy" });
        
        // Create channels for background monitoring
        let (change_tx, change_rx) = broadcast::channel(100);
        
        Self { 
            engine,
            screen_state: Arc::new(RwLock::new(None)),
            update_threshold,
            monitor_tx: None,
            change_rx,
            change_tx,
        }
    }
    
    /// Generate a hash of the image for change detection
    fn hash_image(&self, image: &DynamicImage) -> String {
        let rgb_image = image.to_rgb8();
        let hash = digest::digest(&digest::SHA256, rgb_image.as_raw());
        hex::encode(&hash.as_ref()[..16]) // Use first 16 bytes for shorter hash
    }
    
    /// Check if the current cached state is still valid for this image
    fn is_state_current(&self, image_hash: &str) -> bool {
        if let Some(state) = self.screen_state.read().as_ref() {
            state.image_hash == image_hash && !state.is_stale(self.update_threshold)
        } else {
            false
        }
    }
    
    /// Update the cached screen state
    fn update_screen_state(&self, new_state: ScreenState) {
        let old_state = {
            let mut guard = self.screen_state.write();
            let old = guard.clone();
            *guard = Some(new_state.clone());
            old
        };
        
        trace!("Updated screen state cache with {} chars of text", new_state.text.len());
        
        // Emit change event if this is a meaningful change
        let has_changed = match &old_state {
            Some(old) => old.image_hash != new_state.image_hash || old.text != new_state.text,
            None => true,
        };
        
        if has_changed {
            let event = ScreenChangeEvent {
                old_state,
                new_state,
                timestamp: Instant::now(),
            };
            
            // Try to send change event, but don't fail if no receivers
            let _ = self.change_tx.send(event);
        }
    }
    
    pub async fn extract_text(&self, image: &DynamicImage) -> Result<String> {
        trace!("Extracting text using ocrs ML-based OCR with screen state caching");
        
        // Check if we can use cached screen state
        let image_hash = self.hash_image(image);
        if self.is_state_current(&image_hash) {
            let cached_text = self.screen_state.read().as_ref().unwrap().text.clone();
            trace!("Using cached screen state: '{}'", cached_text);
            return Ok(cached_text);
        }
        
        // Convert to RGB format for ocrs
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        
        trace!("=== SCREEN CAPTURE ANALYSIS ===");
        trace!("Image dimensions: {}x{}", width, height);
        trace!("Image format: {:?}", image.color());
        trace!("RGB image raw data length: {} bytes", rgb_image.as_raw().len());
        
        // Check if image is completely black or white (common issues)
        let pixel_data = rgb_image.as_raw();
        let total_pixels = (width * height) as usize;
        let mut black_pixels = 0;
        let mut white_pixels = 0;
        
        for chunk in pixel_data.chunks_exact(3) {
            if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 {
                black_pixels += 1;
            } else if chunk[0] == 255 && chunk[1] == 255 && chunk[2] == 255 {
                white_pixels += 1;
            }
        }
        
        let black_percentage = (black_pixels * 100) / total_pixels;
        let white_percentage = (white_pixels * 100) / total_pixels;

        trace!("Image analysis: {}% black pixels, {}% white pixels", black_percentage, white_percentage);

        if black_percentage > 95 {
            trace!("Image is {}% black - screen capture may be blank/failed", black_percentage);
        }
        if white_percentage > 95 {
            trace!("Image is {}% white - screen may be blank or not displaying content", white_percentage);
        }
        
        // Save a copy of the screenshot for debugging 
        let debug_path = format!("debug-screenshot-{}.png", std::process::id());
        if let Err(e) = image.save(&debug_path) {
            warn!("Failed to save debug screenshot: {}", e);
        } else {
            trace!("Saved debug screenshot to: {} ({}% black, {}% white)", 
                  debug_path, black_percentage, white_percentage);
        }
        
        trace!("===============================");
        
        // Convert image to tensor format for ocrs 0.10.4
        let in_chans = 3;
        let tensor = rten_tensor::NdTensor::from_data(
            [height as usize, width as usize, in_chans],
            rgb_image.into_vec(),
        );
        
        // Create image source for OCR
        let img_source = ImageSource::from_tensor(tensor.view(), DimOrder::Hwc)
            .context("Failed to create image source")?;
        
        // Prepare input for OCR processing
        let ocr_input = self.engine.prepare_input(img_source)
            .context("Failed to prepare OCR input")?;
        
        // Step 1: Detect word rectangles - handle failures gracefully
        trace!("OCR Step 1: Detecting words...");
        let word_rects = match self.engine.detect_words(&ocr_input) {
            Ok(rects) => {
                trace!("OCR detected {} word regions", rects.len());
                if rects.is_empty() {
                    trace!("Word detection succeeded but found no text regions. Image may be blank or text-free.");
                }
                rects
            }
            Err(e) => {
                trace!("OCR word detection failed: {}. This can happen with dark screens, BIOS, boot screens, or low-contrast images.", e);
                trace!("Continuing with fallback text recognition approaches");
                Vec::new()
            }
        };

        // Step 2: Find text lines from word rectangles
        trace!("OCR Step 2: Finding text lines...");
        let line_rects = if word_rects.is_empty() {
            trace!("No word regions available. Using basic image regions for text detection.");
            Vec::new()
        } else {
            self.engine.find_text_lines(&ocr_input, &word_rects)
        };

        trace!("OCR found {} text lines", line_rects.len());

        // Step 3: Recognize text
        trace!("OCR Step 3: Recognizing text...");
        let line_texts = if !line_rects.is_empty() {
            // Normal case: recognize text in detected lines
            match self.engine.recognize_text(&ocr_input, &line_rects) {
                Ok(texts) => {
                    let successful_recognitions = texts.iter().filter(|t| t.is_some()).count();
                    trace!("Successfully recognized text in {}/{} detected lines", 
                          successful_recognitions, texts.len());
                    texts
                }
                Err(e) => {
                    warn!("Text recognition failed on detected lines: {}. This may indicate corrupted models or unsupported text format.", e);
                    Vec::new()
                }
            }
        } else {
            trace!("No text lines detected. This is normal for screens with minimal text (BIOS, boot screens, etc.)");
            Vec::new()
        };
        
        trace!("OCR recognition returned {} results", line_texts.len());
        
        // Combine all recognized text with better filtering and formatting
        let extracted_text = line_texts
            .iter()
            .flatten()
            .filter_map(|line| {
                let text = line.to_string().trim().to_string();
                // Filter out very short detections and noise
                if text.len() > 1 && !text.chars().all(|c| c.is_whitespace()) {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<String>>()
            .join(" ");
        
        if !extracted_text.is_empty() {
            info!("OCR successfully extracted text: '{}'", extracted_text);
        } else {
            debug!("OCR completed but no readable text was found");
        }
        
        // Provide detailed debugging information
        if extracted_text.is_empty() {
            if black_percentage > 90 {
                trace!("No text detected on predominantly black screen ({}% black). This is normal for:", black_percentage);
                trace!("  - Boot screens, BIOS menus, or splash screens");
                trace!("  - VM startup before OS loads");
                trace!("  - Blank or powered-off displays");
            } else if white_percentage > 90 {
                trace!("No text detected on predominantly white screen ({}% white). Possible scenarios:", white_percentage);
                trace!("  - Blank document or empty desktop");
                trace!("  - Screen saver or locked screen");
                trace!("  - Application with minimal UI");
            } else {
                trace!("No text detected despite screen content ({}% black, {}% white).", black_percentage, white_percentage);
                trace!("This could indicate:");
                trace!("  - Text in unsupported format/language");
                trace!("  - Very low contrast or stylized text");
                trace!("  - Graphics-heavy interface with minimal text");
                trace!("  - OCR model limitations with this content type");
            }
        } else {
            trace!("=== OCR SUCCESS - TEXT DETECTED ===");
            trace!("Extracted text: '{}'", extracted_text);
            trace!("Statistics: {} chars, {} words", 
                  extracted_text.len(), 
                  extracted_text.split_whitespace().count());
            trace!("Screen: {}% black, {}% white", black_percentage, white_percentage);
            trace!("===================================");
        }
        
        // Update the cached screen state
        let new_state = ScreenState {
            text: extracted_text.clone(),
            image_hash,
            timestamp: Instant::now(),
            black_percentage,
            white_percentage,
            dimensions: (width, height),
        };
        self.update_screen_state(new_state);
        
        Ok(extracted_text)
    }
    
    pub async fn contains_text(&self, image: &DynamicImage, pattern: &str) -> Result<bool> {
        let extracted_text = self.extract_text(image).await?;
        debug!("Extracted text: {}", extracted_text);
        
        // Case-insensitive search
        Ok(extracted_text.to_lowercase().contains(&pattern.to_lowercase()))
    }
    
    pub async fn wait_for_text_in_image(&self, image: &DynamicImage, pattern: &str, attempts: u32) -> Result<bool> {
        for attempt in 1..=attempts {
            debug!("OCR attempt {}/{} looking for: {}", attempt, attempts, pattern);
            
            if self.contains_text(image, pattern).await? {
                return Ok(true);
            }
            
            if attempt < attempts {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        
        Ok(false)
    }
    
    /// Get the current cached screen state (if available and not stale)
    pub fn get_current_screen_state(&self) -> Option<ScreenState> {
        self.screen_state.read().clone().filter(|state| !state.is_stale(self.update_threshold))
    }
    
    /// Get the last known text from the screen without performing new OCR
    pub fn get_cached_text(&self) -> Option<String> {
        self.get_current_screen_state().map(|state| state.text)
    }
    
    /// Check if cached screen state contains text pattern (fast lookup)
    pub fn cached_contains_text(&self, pattern: &str) -> Option<bool> {
        self.get_cached_text().map(|text| text.to_lowercase().contains(&pattern.to_lowercase()))
    }
    
    /// Force refresh of screen state with new image
    pub async fn refresh_screen_state(&self, image: &DynamicImage) -> Result<ScreenState> {
        // Clear current cache to force refresh
        *self.screen_state.write() = None;
        
        // Extract text which will update the cache
        let _text = self.extract_text(image).await?;
        
        // Return the new state
        self.get_current_screen_state()
            .ok_or_else(|| anyhow::anyhow!("Failed to update screen state"))
    }
    
    /// Check if the screen appears to have changed significantly
    pub fn has_screen_changed(&self, image: &DynamicImage) -> bool {
        let current_hash = self.hash_image(image);
        
        if let Some(state) = self.screen_state.read().as_ref() {
            state.image_hash != current_hash
        } else {
            true // No previous state, so consider it changed
        }
    }
    
    /// Get screen change status and timing info for debugging
    pub fn get_cache_info(&self) -> Option<(String, Duration, usize)> {
        self.screen_state.read().as_ref().map(|state| {
            (
                state.image_hash.clone(),
                state.timestamp.elapsed(),
                state.text.len()
            )
        })
    }
    
    /// Start background screen monitoring with provided screenshot capture
    pub async fn start_monitoring<T: ScreenshotCapture + 'static>(
        &mut self,
        capture: T,
        interval: Duration,
    ) -> Result<()> {
        if self.monitor_tx.is_some() {
            return Err(anyhow::anyhow!("Background monitoring is already running"));
        }
        
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.monitor_tx = Some(command_tx);
        
        // Clone necessary data for the monitoring task
        let screen_state = self.screen_state.clone();
        let change_tx = self.change_tx.clone();
        let update_threshold = self.update_threshold;
        let capture = Arc::new(capture);
        
        // Create a simplified OCR engine for the background task
        let engine_clone = OcrsEngine::new(OcrEngineParams {
            detection_model: Some(load_model(ModelSource::Url(DETECTION_MODEL.to_string()))?),
            recognition_model: Some(load_model(ModelSource::Url(RECOGNITION_MODEL.to_string()))?),
            decode_method: DecodeMethod::Greedy, // Use greedy for background (faster)
            debug: false,
            alphabet: None,
            allowed_chars: None,
            ..Default::default()
        })?;
        
        // Spawn the monitoring task
        tokio::spawn(async move {
            Self::background_monitor_task(
                engine_clone,
                capture,
                command_rx,
                screen_state,
                change_tx,
                interval,
                update_threshold,
            ).await;
        });
        
        info!("Started background screen monitoring with {}ms interval", interval.as_millis());
        Ok(())
    }
    
    /// Stop background monitoring
    pub async fn stop_monitoring(&mut self) -> Result<()> {
        if let Some(tx) = self.monitor_tx.take() {
            let _ = tx.send(MonitorCommand::Shutdown);
            info!("Stopped background screen monitoring");
            Ok(())
        } else {
            Err(anyhow::anyhow!("Background monitoring is not running"))
        }
    }
    
    /// Pause background monitoring
    pub async fn pause_monitoring(&self) -> Result<()> {
        if let Some(tx) = &self.monitor_tx {
            tx.send(MonitorCommand::Pause)?;
            info!("Paused background screen monitoring");
            Ok(())
        } else {
            Err(anyhow::anyhow!("Background monitoring is not running"))
        }
    }
    
    /// Resume background monitoring
    pub async fn resume_monitoring(&self) -> Result<()> {
        if let Some(tx) = &self.monitor_tx {
            tx.send(MonitorCommand::Resume)?;
            info!("Resumed background screen monitoring");
            Ok(())
        } else {
            Err(anyhow::anyhow!("Background monitoring is not running"))
        }
    }
    
    /// Update monitoring interval
    pub async fn update_monitoring_interval(&self, interval: Duration) -> Result<()> {
        if let Some(tx) = &self.monitor_tx {
            tx.send(MonitorCommand::UpdateInterval(interval))?;
            info!("Updated monitoring interval to {}ms", interval.as_millis());
            Ok(())
        } else {
            Err(anyhow::anyhow!("Background monitoring is not running"))
        }
    }
    
    /// Get monitoring status
    pub async fn get_monitoring_status(&self) -> Result<MonitorStatus> {
        if let Some(tx) = &self.monitor_tx {
            let (response_tx, response_rx) = oneshot::channel();
            tx.send(MonitorCommand::GetStatus(response_tx))?;
            
            match tokio::time::timeout(Duration::from_millis(1000), response_rx).await {
                Ok(Ok(status)) => Ok(status),
                Ok(Err(_)) => Err(anyhow::anyhow!("Failed to get monitoring status")),
                Err(_) => Err(anyhow::anyhow!("Timeout waiting for monitoring status")),
            }
        } else {
            Ok(MonitorStatus {
                is_running: false,
                is_paused: false,
                current_interval: Duration::from_millis(0),
                last_update: None,
                total_updates: 0,
                current_state: self.get_current_screen_state(),
            })
        }
    }
    
    /// Subscribe to screen change events
    pub fn subscribe_to_changes(&self) -> broadcast::Receiver<ScreenChangeEvent> {
        self.change_tx.subscribe()
    }
    
    /// Background monitoring task
    async fn background_monitor_task(
        engine: OcrsEngine,
        capture: Arc<dyn ScreenshotCapture>,
        mut command_rx: mpsc::UnboundedReceiver<MonitorCommand>,
        screen_state: Arc<RwLock<Option<ScreenState>>>,
        change_tx: broadcast::Sender<ScreenChangeEvent>,
        mut interval: Duration,
        update_threshold: Duration,
    ) {
        let mut is_paused = false;
        let mut total_updates = 0u64;
        let mut last_update: Option<Instant> = None;
        
        info!("Background screen monitoring task started");
        
        loop {
            // Check for commands with timeout
            tokio::select! {
                command = command_rx.recv() => {
                    match command {
                        Some(MonitorCommand::Pause) => {
                            is_paused = true;
                            trace!("Background monitoring paused");
                        }
                        Some(MonitorCommand::Resume) => {
                            is_paused = false;
                            trace!("Background monitoring resumed");
                        }
                        Some(MonitorCommand::UpdateInterval(new_interval)) => {
                            interval = new_interval;
                            trace!("Background monitoring interval updated to {}ms", interval.as_millis());
                        }
                        Some(MonitorCommand::Shutdown) => {
                            info!("Background monitoring shutting down");
                            break;
                        }
                        Some(MonitorCommand::GetStatus(response_tx)) => {
                            let status = MonitorStatus {
                                is_running: true,
                                is_paused,
                                current_interval: interval,
                                last_update,
                                total_updates,
                                current_state: screen_state.read().clone(),
                            };
                            let _ = response_tx.send(status);
                        }
                        None => {
                            warn!("Command channel closed, shutting down background monitoring");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(if is_paused { Duration::from_secs(1) } else { interval }) => {
                    if !is_paused {
                        // Perform screen capture and OCR
                        match Self::monitor_screen_update(&engine, &capture, &screen_state, &change_tx, update_threshold).await {
                            Ok(updated) => {
                                if updated {
                                    total_updates += 1;
                                    last_update = Some(Instant::now());
                                }
                            }
                            Err(e) => {
                                warn!("Background screen monitoring error: {}", e);
                                // Continue monitoring despite errors
                            }
                        }
                    }
                }
            }
        }
        
        info!("Background screen monitoring task ended");
    }
    
    /// Perform a single screen monitoring update
    async fn monitor_screen_update(
        engine: &OcrsEngine,
        capture: &Arc<dyn ScreenshotCapture>,
        screen_state: &Arc<RwLock<Option<ScreenState>>>,
        change_tx: &broadcast::Sender<ScreenChangeEvent>,
        update_threshold: Duration,
    ) -> Result<bool> {
        // Capture screenshot
        let image = capture.capture().await?;
        
        // Generate hash for change detection
        let rgb_image = image.to_rgb8();
        let hash = digest::digest(&digest::SHA256, rgb_image.as_raw());
        let image_hash = hex::encode(&hash.as_ref()[..16]);
        
        // Check if screen has actually changed
        let needs_update = {
            let state_guard = screen_state.read();
            match state_guard.as_ref() {
                Some(current) => {
                    current.image_hash != image_hash || current.is_stale(update_threshold)
                }
                None => true,
            }
        };
        
        if !needs_update {
            trace!("Screen unchanged, skipping OCR");
            return Ok(false);
        }
        
        // Perform OCR
        let (width, height) = rgb_image.dimensions();
        let pixel_data = rgb_image.as_raw();
        let total_pixels = (width * height) as usize;
        let mut black_pixels = 0;
        let mut white_pixels = 0;
        
        for chunk in pixel_data.chunks_exact(3) {
            if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 {
                black_pixels += 1;
            } else if chunk[0] == 255 && chunk[1] == 255 && chunk[2] == 255 {
                white_pixels += 1;
            }
        }
        
        let black_percentage = (black_pixels * 100) / total_pixels;
        let white_percentage = (white_pixels * 100) / total_pixels;
        
        // Create image source and perform OCR
        let in_chans = 3;
        let tensor = rten_tensor::NdTensor::from_data(
            [height as usize, width as usize, in_chans],
            rgb_image.into_vec(),
        );
        
        let img_source = ImageSource::from_tensor(tensor.view(), DimOrder::Hwc)?;
        let ocr_input = engine.prepare_input(img_source)?;
        
        // Fast OCR processing for background monitoring
        let extracted_text = match engine.detect_words(&ocr_input) {
            Ok(word_rects) if !word_rects.is_empty() => {
                let line_rects = engine.find_text_lines(&ocr_input, &word_rects);
                if !line_rects.is_empty() {
                    match engine.recognize_text(&ocr_input, &line_rects) {
                        Ok(line_texts) => {
                            line_texts
                                .iter()
                                .flatten()
                                .filter_map(|line| {
                                    let text = line.to_string().trim().to_string();
                                    if text.len() > 1 && !text.chars().all(|c| c.is_whitespace()) {
                                        Some(text)
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<String>>()
                                .join(" ")
                        }
                        Err(_) => String::new(),
                    }
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        
        // Create new screen state
        let new_state = ScreenState {
            text: extracted_text,
            image_hash,
            timestamp: Instant::now(),
            black_percentage,
            white_percentage,
            dimensions: (width, height),
        };
        
        // Update state and emit change event
        let old_state = {
            let mut guard = screen_state.write();
            let old = guard.clone();
            *guard = Some(new_state.clone());
            old
        };
        
        let has_changed = match &old_state {
            Some(old) => old.image_hash != new_state.image_hash || old.text != new_state.text,
            None => true,
        };
        
        if has_changed {
            let event = ScreenChangeEvent {
                old_state,
                new_state: new_state.clone(),
                timestamp: Instant::now(),
            };
            
            let _ = change_tx.send(event);
            trace!("Screen state updated: '{}' ({}x{})", new_state.text, width, height);
        }
        
        Ok(has_changed)
    }
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}