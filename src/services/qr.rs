use qrcode::render::svg;
use qrcode::QrCode;

use crate::error::AppError;

pub fn render_svg(target_url: &str) -> Result<String, AppError> {
    let code = QrCode::new(target_url.as_bytes())
        .map_err(|e| AppError::Internal(format!("QR encode failed: {e}")))?;
    let svg = code
        .render::<svg::Color<'_>>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#0e1525"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(svg)
}
