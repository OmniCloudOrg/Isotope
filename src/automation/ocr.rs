use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use tracing::{debug, info, trace, warn};
use ocrs::{OcrEngine as OcrsEngine, OcrEngineParams, ImageSource, DecodeMethod, DimOrder};
use rten_tensor::AsView;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ring::digest;
use tokio::sync::{mpsc, oneshot, broadcast};
use async_trait::async_trait;
use std::sync::LazyLock;

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
    /// Adaptive timeout tracking for OCR operations
    timeout_tracker: Arc<RwLock<TimeoutTracker>>,
}

/// Tracks OCR timeouts and adapts timeout duration based on failure patterns
#[derive(Debug, Clone)]
struct TimeoutTracker {
    /// Current timeout duration (starts at 10s, increases with failures)
    current_timeout: Duration,
    /// Number of consecutive timeout failures
    consecutive_failures: u32,
    /// Maximum timeout (30s)
    max_timeout: Duration,
    /// Base timeout (10s)
    base_timeout: Duration,
    /// Last successful OCR timestamp
    last_success: Option<Instant>,
}

impl TimeoutTracker {
    fn new() -> Self {
        Self {
            current_timeout: Duration::from_secs(10),
            consecutive_failures: 0,
            max_timeout: Duration::from_secs(30),
            base_timeout: Duration::from_secs(10),
            last_success: None,
        }
    }
    
    /// Record a successful OCR operation
    fn record_success(&mut self) {
        self.last_success = Some(Instant::now());
        self.consecutive_failures = 0;
        // Gradually reduce timeout back toward base timeout
        if self.current_timeout > self.base_timeout {
            let reduction = Duration::from_secs(2);
            self.current_timeout = self.current_timeout.saturating_sub(reduction).max(self.base_timeout);
        }
    }
    
    /// Record a timeout failure and increase timeout for next attempt
    fn record_timeout(&mut self) {
        self.consecutive_failures += 1;
        // Increase timeout: 10s -> 15s -> 20s -> 25s -> 30s (max)
        let increase = Duration::from_secs(5 * self.consecutive_failures as u64);
        self.current_timeout = (self.base_timeout + increase).min(self.max_timeout);
    }
    
    /// Get current timeout duration
    fn get_timeout(&self) -> Duration {
        self.current_timeout
    }
}

/// Default text detection model.
const DETECTION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";

/// Default text recognition model.
const RECOGNITION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

/// Cached model paths to avoid repeated downloads
static CACHED_DETECTION_PATH: LazyLock<Result<std::path::PathBuf, anyhow::Error>> = LazyLock::new(|| {
    info!("Downloading and caching text detection model...");
    super::models::download_file(DETECTION_MODEL, None)
});

static CACHED_RECOGNITION_PATH: LazyLock<Result<std::path::PathBuf, anyhow::Error>> = LazyLock::new(|| {
    info!("Downloading and caching text recognition model...");
    super::models::download_file(RECOGNITION_MODEL, None)
});

impl OcrEngine {
    pub fn new() -> Self {
        Self::with_options(false, Duration::from_millis(100))
    }
    
    pub fn with_beam_search() -> Self {
        Self::with_options(true, Duration::from_millis(100))
    }
    
    pub fn with_update_threshold(threshold: Duration) -> Self {
        Self::with_options(false, threshold)
    }
    
    fn with_options(beam_search: bool, update_threshold: Duration) -> Self {
        debug!("Initializing enhanced OCR engine using cached pre-trained models");
        
        // Use cached model paths to avoid repeated downloads, but still load models fresh
        let detection_binding = CACHED_DETECTION_PATH.as_ref();
        let detection_path = detection_binding
            .as_ref()
            .expect("Failed to get cached detection model path");
        let recognition_binding = CACHED_RECOGNITION_PATH.as_ref();
        let recognition_path = recognition_binding
            .as_ref()
            .expect("Failed to get cached recognition model path");
        
        let detection_model = load_model(ModelSource::Path(detection_path.to_string_lossy().to_string()))
            .expect("Failed to load detection model from cached path");
        let recognition_model = load_model(ModelSource::Path(recognition_path.to_string_lossy().to_string()))
            .expect("Failed to load recognition model from cached path");
        
        // Create OCR engine with enhanced parameters
        let decode_method = if beam_search {
            DecodeMethod::BeamSearch { width: 500 }
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
        
        debug!("OCR engine initialized with cached models and {} decoding", 
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
            timeout_tracker: Arc::new(RwLock::new(TimeoutTracker::new())),
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
            // Only use cache if image hash matches AND it's very recent (less than 200ms)
            // This makes caching less aggressive so we see more OCR processing
            state.image_hash == image_hash && !state.is_stale(Duration::from_millis(200))
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
        let start_time = Instant::now();
        debug!("OCR extract_text called");
        
        // Check if we can use cached screen state
        let image_hash = self.hash_image(image);
        if self.is_state_current(&image_hash) {
            let cached_text = self.screen_state.read().as_ref().unwrap().text.clone();
            debug!("OCR cache hit ({}ms) - using recent screen state: '{}'", start_time.elapsed().as_millis(), cached_text);
            return Ok(cached_text);
        }
        
        debug!("OCR cache miss - performing fresh text extraction");
        
        // Get current timeout duration
        let timeout_duration = self.timeout_tracker.read().get_timeout();
        debug!("Using OCR timeout of {}s", timeout_duration.as_secs());
        
        // Wrap OCR processing in timeout
        match tokio::time::timeout(timeout_duration, self.extract_text_internal(image, image_hash.clone())).await {
            Ok(Ok(text)) => {
                // Success - record it and return result
                self.timeout_tracker.write().record_success();
                if !text.is_empty() {
                    info!("OCR SUCCESS with {}s timeout ({}ms): '{}'", 
                          timeout_duration.as_secs(), start_time.elapsed().as_millis(), text);
                }
                Ok(text)
            }
            Ok(Err(e)) => {
                // OCR error (not timeout)
                warn!("OCR processing error: {}", e);
                Err(e)
            }
            Err(_) => {
                // Timeout occurred
                self.timeout_tracker.write().record_timeout();
                let new_timeout = self.timeout_tracker.read().get_timeout();
                warn!("OCR timed out after {}s, next timeout will be {}s", 
                      timeout_duration.as_secs(), new_timeout.as_secs());
                
                // Return empty result to allow system to try next frame
                let empty_state = ScreenState {
                    text: String::new(),
                    image_hash,
                    timestamp: Instant::now(),
                    black_percentage: 0,
                    white_percentage: 0,
                    dimensions: image.dimensions(),
                };
                self.update_screen_state(empty_state);
                Ok(String::new())
            }
        }
    }
    
    /// Internal OCR processing without timeout wrapper
    async fn extract_text_internal(&self, image: &DynamicImage, image_hash: String) -> Result<String> {
        // Convert to RGB format for ocrs (reuse existing if possible)
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        
        debug!("Processing {}x{} image", width, height);
        
        // Fast pixel analysis using sampling for better performance
        let pixel_data = rgb_image.as_raw();
        let total_pixels = (width * height) as usize;
        
        // Ultra-fast sampling: every 500th pixel for maximum speed
        let sample_size = (total_pixels / 500).max(200); // At least 200 samples
        let step = (total_pixels / sample_size).max(1);
        
        let mut black_pixels = 0;
        let mut white_pixels = 0;
        let mut sample_count = 0;
        
        for i in (0..pixel_data.len()).step_by(step * 3) {
            if i + 2 < pixel_data.len() {
                let r = pixel_data[i];
                let g = pixel_data[i + 1];
                let b = pixel_data[i + 2];
                
                if r == 0 && g == 0 && b == 0 {
                    black_pixels += 1;
                } else if r == 255 && g == 255 && b == 255 {
                    white_pixels += 1;
                }
                sample_count += 1;
            }
        }
        
        let black_percentage = (black_pixels * 100) / sample_count.max(1);
        let white_percentage = (white_pixels * 100) / sample_count.max(1);

        debug!("Fast pixel analysis: {}% black, {}% white (sampled {} pixels)", 
               black_percentage, white_percentage, sample_count);
        
        // Fast-path for obviously empty screens - skip expensive OCR
        if black_percentage > 95 {
            debug!("Fast-path: predominantly black screen ({}%), skipping OCR", black_percentage);
            let empty_state = ScreenState {
                text: String::new(),
                image_hash,
                timestamp: Instant::now(),
                black_percentage,
                white_percentage,
                dimensions: (width, height),
            };
            self.update_screen_state(empty_state);
            debug!("Fast-path: empty black screen detected");
            return Ok(String::new());
        }
        
        if white_percentage > 95 {
            debug!("Fast-path: predominantly white screen ({}%), skipping OCR", white_percentage);
            let empty_state = ScreenState {
                text: String::new(),
                image_hash,
                timestamp: Instant::now(),
                black_percentage,
                white_percentage,
                dimensions: (width, height),
            };
            self.update_screen_state(empty_state);
            debug!("Fast-path: empty white screen detected");
            return Ok(String::new());
        }
        
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
        
        // Ultra-fast OCR processing - optimized single-pass approach
        trace!("Starting optimized OCR processing...");
        
        // Try fast approach: detect words then immediately recognize (skip line finding)
        let word_rects = match self.engine.detect_words(&ocr_input) {
            Ok(rects) => rects,
            Err(e) => {
                trace!("Word detection failed: {}", e);
                return Ok(String::new()); // Exit early if detection fails
            }
        };
        
        if word_rects.is_empty() {
            trace!("No words detected");
            return Ok(String::new());
        }
        
        // Use traditional line-based approach but optimize it
        let line_rects = self.engine.find_text_lines(&ocr_input, &word_rects);
        
        if line_rects.is_empty() {
            trace!("No text lines found");
            return Ok(String::new());
        }
        
        let line_texts = match self.engine.recognize_text(&ocr_input, &line_rects) {
            Ok(texts) => texts,
            Err(e) => {
                trace!("Text recognition failed: {}", e);
                return Ok(String::new());
            }
        };
        
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
            debug!("OCR text extraction completed: '{}'", extracted_text);
        } else {
            debug!("OCR completed but no text found ({}% black, {}% white)", 
                  black_percentage, white_percentage);
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
                        debug!("Background monitor performing screen check (update #{})", total_updates + 1);
                        match Self::monitor_screen_update(&engine, &capture, &screen_state, &change_tx, update_threshold).await {
                            Ok(updated) => {
                                if updated {
                                    total_updates += 1;
                                    last_update = Some(Instant::now());
                                    info!("Background monitor detected screen change #{} - updated state", total_updates);
                                } else {
                                    debug!("Background monitor found no screen changes");
                                }
                            }
                            Err(e) => {
                                warn!("Background screen monitoring error: {}", e);
                                // Continue monitoring despite errors
                            }
                        }
                        
                        // Provide periodic status updates
                        if total_updates % 10 == 0 && total_updates > 0 {
                            let current_text = {
                                let guard = screen_state.read();
                                guard.as_ref()
                                    .map(|s| s.text.clone())
                                    .unwrap_or_else(|| "(no state)".to_string())
                            };
                            info!("Background monitor status: {} updates, current screen: '{}'", 
                                  total_updates, current_text);
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
            debug!("Background monitor: screen unchanged, skipping OCR");
            return Ok(false);
        }
        
        debug!("Background monitor: screen changed, performing OCR");
        
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
            info!("Background monitor extracted: '{}' ({}x{})", new_state.text, width, height);
        }
        
        Ok(has_changed)
    }
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}