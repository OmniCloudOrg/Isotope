use anyhow::{Context, Result};
use image::DynamicImage;
use tracing::{debug, info, warn};
use ocrs::{OcrEngine as OcrsEngine, OcrEngineParams, ImageSource, DecodeMethod, DimOrder};
use rten_tensor::AsView;

use super::models::{load_model, ModelSource};

pub struct OcrEngine {
    engine: OcrsEngine,
}

/// Default text detection model.
const DETECTION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten";

/// Default text recognition model.
const RECOGNITION_MODEL: &str = "https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten";

impl OcrEngine {
    pub fn new() -> Self {
        Self::with_options(false)
    }
    
    pub fn with_beam_search() -> Self {
        Self::with_options(true)
    }
    
    fn with_options(beam_search: bool) -> Self {
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
        info!("OCR Step 1: Detecting words...");
        let word_rects = match self.engine.detect_words(&ocr_input) {
            Ok(rects) => {
                info!("OCR detected {} word regions", rects.len());
                if rects.is_empty() {
                    warn!("Word detection succeeded but found no text regions. Image may be blank or text-free.");
                }
                rects
            }
            Err(e) => {
                warn!("OCR word detection failed: {}. This can happen with dark screens, BIOS, boot screens, or low-contrast images.", e);
                info!("Continuing with fallback text recognition approaches");
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
        
        // Step 3: Recognize text
        info!("OCR Step 3: Recognizing text...");
        let line_texts = if !line_rects.is_empty() {
            // Normal case: recognize text in detected lines
            match self.engine.recognize_text(&ocr_input, &line_rects) {
                Ok(texts) => {
                    let successful_recognitions = texts.iter().filter(|t| t.is_some()).count();
                    info!("Successfully recognized text in {}/{} detected lines", 
                          successful_recognitions, texts.len());
                    texts
                }
                Err(e) => {
                    warn!("Text recognition failed on detected lines: {}. This may indicate corrupted models or unsupported text format.", e);
                    Vec::new()
                }
            }
        } else {
            info!("No text lines detected. This is normal for screens with minimal text (BIOS, boot screens, etc.)");
            Vec::new()
        };
        
        info!("OCR recognition returned {} results", line_texts.len());
        
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
                info!("No text detected on predominantly black screen ({}% black). This is normal for:", black_percentage);
                info!("  - Boot screens, BIOS menus, or splash screens");
                info!("  - VM startup before OS loads");
                info!("  - Blank or powered-off displays");
            } else if white_percentage > 90 {
                info!("No text detected on predominantly white screen ({}% white). Possible scenarios:", white_percentage);
                info!("  - Blank document or empty desktop");
                info!("  - Screen saver or locked screen");
                info!("  - Application with minimal UI");
            } else {
                warn!("No text detected despite screen content ({}% black, {}% white).", black_percentage, white_percentage);
                info!("This could indicate:");
                info!("  - Text in unsupported format/language");
                info!("  - Very low contrast or stylized text");
                info!("  - Graphics-heavy interface with minimal text");
                info!("  - OCR model limitations with this content type");
            }
        } else {
            info!("=== OCR SUCCESS - TEXT DETECTED ===");
            info!("Extracted text: '{}'", extracted_text);
            info!("Statistics: {} chars, {} words", 
                  extracted_text.len(), 
                  extracted_text.split_whitespace().count());
            info!("Screen: {}% black, {}% white", black_percentage, white_percentage);
            info!("===================================");
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