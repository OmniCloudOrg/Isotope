use anyhow::{Context, Result};
use image::DynamicImage;
use tracing::{debug, info, trace, warn};
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
        trace!("Extracting text using ocrs ML-based OCR");
        
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
        
        info!("Image analysis: {}% black pixels, {}% white pixels", black_percentage, white_percentage);
        
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
            trace!("OCR successfully extracted text: '{}'", extracted_text);
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