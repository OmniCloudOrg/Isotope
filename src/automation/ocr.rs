use anyhow::{anyhow, Context, Result};
use image::DynamicImage;
use std::process::Command;
use std::io::Write;
use tempfile::NamedTempFile;
use tracing::{debug, warn};

pub struct OcrEngine {
    engine_type: OcrEngineType,
}

#[derive(Debug, Clone)]
pub enum OcrEngineType {
    Tesseract,
    WindowsOcr,
    Fallback,
}

impl OcrEngine {
    pub fn new() -> Self {
        // Try to detect available OCR engines
        let engine_type = Self::detect_available_engine();
        debug!("Using OCR engine: {:?}", engine_type);
        
        Self { engine_type }
    }
    
    fn detect_available_engine() -> OcrEngineType {
        // First try Tesseract
        if Self::is_tesseract_available() {
            return OcrEngineType::Tesseract;
        }
        
        // On Windows, try Windows OCR
        #[cfg(windows)]
        {
            return OcrEngineType::WindowsOcr;
        }
        
        // Fallback to simple pattern matching
        OcrEngineType::Fallback
    }
    
    fn is_tesseract_available() -> bool {
        Command::new("tesseract")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
    
    pub async fn extract_text(&self, image: &DynamicImage) -> Result<String> {
        match self.engine_type {
            OcrEngineType::Tesseract => self.tesseract_ocr(image).await,
            OcrEngineType::WindowsOcr => self.windows_ocr(image).await,
            OcrEngineType::Fallback => self.fallback_ocr(image).await,
        }
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
    
    async fn tesseract_ocr(&self, image: &DynamicImage) -> Result<String> {
        debug!("Using Tesseract OCR");
        
        // Save image to temporary file
        let mut temp_file = NamedTempFile::new()
            .context("Failed to create temporary file")?;
        
        // Save as PNG
        image.save_with_format(temp_file.path(), image::ImageFormat::Png)
            .context("Failed to save image to temporary file")?;
        
        // Run tesseract
        let output = Command::new("tesseract")
            .arg(temp_file.path())
            .arg("stdout")
            .arg("-l")
            .arg("eng")
            .output()
            .context("Failed to execute tesseract")?;
        
        if !output.status.success() {
            return Err(anyhow!("Tesseract failed: {}", 
                String::from_utf8_lossy(&output.stderr)));
        }
        
        let text = String::from_utf8(output.stdout)
            .context("Tesseract output is not valid UTF-8")?;
        
        Ok(text.trim().to_string())
    }
    
    async fn windows_ocr(&self, _image: &DynamicImage) -> Result<String> {
        #[cfg(windows)]
        {
            // This would use Windows.Media.Ocr API via PowerShell or direct WinRT calls
            // For now, implement a simple PowerShell-based OCR
            warn!("Windows OCR not fully implemented, using fallback");
            self.fallback_ocr(_image).await
        }
        
        #[cfg(not(windows))]
        {
            Err(anyhow!("Windows OCR not available on this platform"))
        }
    }
    
    async fn fallback_ocr(&self, image: &DynamicImage) -> Result<String> {
        debug!("Using fallback OCR (simple pattern detection)");
        
        // This is a very simple fallback that looks for common patterns
        // In a production system, you'd want a proper OCR library
        
        // Convert to grayscale and analyze pixel patterns
        let gray_image = image.to_luma8();
        let (width, height) = gray_image.dimensions();
        
        // Simple heuristics for common text patterns
        let mut detected_patterns = Vec::new();
        
        // Look for horizontal lines (potential text baselines)
        let mut horizontal_lines = 0;
        for y in 0..height {
            let mut line_pixels = 0;
            for x in 0..width {
                let pixel = gray_image.get_pixel(x, y);
                if pixel[0] < 128 { // Dark pixel
                    line_pixels += 1;
                }
            }
            if line_pixels > width / 4 {
                horizontal_lines += 1;
            }
        }
        
        // Basic pattern detection for common boot/login screens
        if horizontal_lines > 5 {
            detected_patterns.push("text_detected");
        }
        
        // Look for common login screen patterns
        if self.has_login_pattern(&gray_image) {
            detected_patterns.push("login");
        }
        
        if self.has_desktop_pattern(&gray_image) {
            detected_patterns.push("desktop");
        }
        
        Ok(detected_patterns.join(" "))
    }
    
    fn has_login_pattern(&self, image: &image::GrayImage) -> bool {
        // Simple heuristic: login screens often have centered text areas
        let (width, height) = image.dimensions();
        let center_x = width / 2;
        let center_y = height / 2;
        
        // Check for dark regions in the center (text input boxes)
        let mut dark_pixels_center = 0;
        let check_size = 50;
        
        for y in (center_y.saturating_sub(check_size))..=(center_y + check_size).min(height - 1) {
            for x in (center_x.saturating_sub(check_size))..=(center_x + check_size).min(width - 1) {
                let pixel = image.get_pixel(x, y);
                if pixel[0] < 100 {
                    dark_pixels_center += 1;
                }
            }
        }
        
        // If there's a significant concentration of dark pixels in center, might be login
        dark_pixels_center > (check_size * check_size) / 4
    }
    
    fn has_desktop_pattern(&self, image: &image::GrayImage) -> bool {
        // Desktop patterns often have icons or taskbars at edges
        let (width, height) = image.dimensions();
        
        // Check bottom edge for taskbar (often darker than background)
        let mut bottom_dark_pixels = 0;
        let taskbar_height = 50;
        
        for y in (height.saturating_sub(taskbar_height))..height {
            for x in 0..width {
                let pixel = image.get_pixel(x, y);
                if pixel[0] < 150 {
                    bottom_dark_pixels += 1;
                }
            }
        }
        
        // If bottom area has many dark pixels, might be desktop with taskbar
        bottom_dark_pixels > (width * taskbar_height) / 3
    }
}

impl Default for OcrEngine {
    fn default() -> Self {
        Self::new()
    }
}