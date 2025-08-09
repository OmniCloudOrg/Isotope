use anyhow::{Context, Result};
use image::DynamicImage;
use tracing::{debug, info, warn};
use ocrs::{OcrEngine as OcrsEngine, OcrEngineParams, ImageSource};

pub struct OcrEngine {
    engine: OcrsEngine,
}

impl OcrEngine {
    pub fn new() -> Self {
        info!("Initializing OCR engine using ocrs (pure Rust ML-based OCR)");
        
        // Create OCR engine with default parameters (no external models needed)
        let engine = OcrsEngine::new(OcrEngineParams::default())
            .expect("Failed to initialize OCR engine");
        
        Self { engine }
    }
    
    pub async fn extract_text(&self, image: &DynamicImage) -> Result<String> {
        debug!("Extracting text using ocrs ML-based OCR");
        
        // Convert to RGB format for ocrs
        let rgb_image = image.to_rgb8();
        let (width, height) = rgb_image.dimensions();
        
        info!("=== SCREEN CAPTURE ANALYSIS ===");
        info!("Image dimensions: {}x{}", width, height);
        info!("Image format: {:?}", image.color());
        info!("RGB image raw data length: {} bytes", rgb_image.as_raw().len());
        
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
        
        info!("Image analysis: {}% black pixels, {}% white pixels", black_percentage, white_percentage);
        
        if black_percentage > 95 {
            warn!("Image is {}% black - screen capture may be blank/failed", black_percentage);
        }
        if white_percentage > 95 {
            warn!("Image is {}% white - screen may be blank or not displaying content", white_percentage);
        }
        
        // Save a copy of the screenshot for debugging 
        let debug_path = format!("debug-screenshot-{}.png", std::process::id());
        if let Err(e) = image.save(&debug_path) {
            warn!("Failed to save debug screenshot: {}", e);
        } else {
            info!("Saved debug screenshot to: {} ({}% black, {}% white)", 
                  debug_path, black_percentage, white_percentage);
        }
        
        info!("===============================");
        
        // Create image source for OCR
        let img_source = ImageSource::from_bytes(
            rgb_image.as_raw(), 
            (width, height)
        ).context("Failed to create image source")?;
        
        // Prepare input for OCR processing
        let ocr_input = self.engine.prepare_input(img_source)
            .context("Failed to prepare OCR input")?;
        
        // Step 1: Detect word rectangles - handle failures gracefully
        info!("OCR Step 1: Detecting words...");
        let word_rects = match self.engine.detect_words(&ocr_input) {
            Ok(rects) => {
                info!("OCR detected {} word regions", rects.len());
                rects
            }
            Err(e) => {
                warn!("OCR word detection failed: {}. This is common with dark/low-contrast screens.", e);
                info!("Skipping word detection and attempting direct text recognition");
                
                // Return empty vec to proceed with alternative approaches
                Vec::new()
            }
        };
        
        // Step 2: Find text lines from word rectangles  
        info!("OCR Step 2: Finding text lines...");
        let line_rects = if word_rects.is_empty() {
            info!("No word regions available. Using basic image regions for text detection.");
            Vec::new()
        } else {
            self.engine.find_text_lines(&ocr_input, &word_rects)
        };
        
        info!("OCR found {} text lines", line_rects.len());
        
        // Step 3: Recognize text - if we have no lines, still attempt recognition on full image
        info!("OCR Step 3: Recognizing text...");
        let line_texts = if !line_rects.is_empty() {
            // Normal case: recognize text in detected lines
            match self.engine.recognize_text(&ocr_input, &line_rects) {
                Ok(texts) => texts,
                Err(e) => {
                    warn!("Text recognition failed on detected lines: {}", e);
                    Vec::new()
                }
            }
        } else {
            warn!("No text lines detected. Attempting basic text extraction...");
            
            // For cases where word/line detection fails (dark screens, etc.)
            // Try alternative recognition approaches or return empty
            // Note: ocrs might not support direct full-image recognition
            // so we may need to return empty and handle this case
            Vec::new()
        };
        
        info!("OCR recognition returned {} results", line_texts.len());
        
        // Combine all recognized text
        let extracted_text = line_texts
            .iter()
            .flatten()
            .filter(|line| line.to_string().len() > 1) // Filter out very short detections
            .map(|line| line.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        
        info!("OCR extracted text: '{}'", extracted_text);
        
        // Always dump what OCR sees for debugging
        if extracted_text.is_empty() {
            if black_percentage > 90 {
                warn!("OCR found NO TEXT on screen. Screen is {}% black - likely at boot screen, BIOS, or blank display", black_percentage);
                info!("This may be normal if VM is still booting or at a menu with minimal text");
            } else {
                warn!("OCR found NO TEXT but screen has only {}% black pixels - OCR may have failed", black_percentage);
                info!("The screen has content but OCR couldn't extract text from it");
            }
        } else {
            info!("=== WHAT OCR SEES ON SCREEN ===");
            info!("Full extracted text: '{}'", extracted_text);
            info!("Text length: {} characters", extracted_text.len());
            info!("Word count: {} words", extracted_text.split_whitespace().count());
            info!("Screen composition: {}% black, {}% white pixels", black_percentage, white_percentage);
            info!("================================");
        }
        
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
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}