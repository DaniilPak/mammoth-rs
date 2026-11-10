//! Image converters: decide how [`Image`]s are written to HTML.

use std::fmt;
use std::sync::Arc;

use crate::document::Image;

/// Error type returned by image conversion functions.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

type ConvertFn = dyn Fn(&Image) -> Result<Vec<(String, String)>, BoxError> + Send + Sync;

/// Converts an [`Image`] into the attributes of an `<img>` element.
///
/// The default converter embeds images as `data:` URIs. Use
/// [`ImageConverter::img_element`] to store images elsewhere, for instance
/// to write them to disk and reference them by URL:
///
/// ```
/// use mammoth_rs::{ImageConverter, Options};
///
/// let options = Options::new().image_converter(ImageConverter::img_element(|image| {
///     let bytes = image.read()?;
///     let src = format!("images/{}-bytes.png", bytes.len());
///     Ok(vec![("src".to_string(), src)])
/// }));
/// ```
///
/// If the image has alt text, an `alt` attribute is added first; attributes
/// returned by the converter take precedence over it. Errors are reported as
/// [`MessageKind::Error`](crate::MessageKind::Error) messages and the image is
/// omitted.
#[derive(Clone)]
pub struct ImageConverter(Arc<ConvertFn>);

impl ImageConverter {
    /// Creates a converter from a function returning `<img>` attributes.
    pub fn img_element<F>(convert: F) -> Self
    where
        F: Fn(&Image) -> Result<Vec<(String, String)>, BoxError> + Send + Sync + 'static,
    {
        ImageConverter(Arc::new(convert))
    }

    /// The default converter: `src` is a base64 `data:` URI.
    pub fn data_uri() -> Self {
        ImageConverter::img_element(|image| {
            let content_type = image.content_type.as_deref().unwrap_or("null");
            let encoded = image.read_as_base64()?;
            Ok(vec![(
                "src".to_string(),
                format!("data:{content_type};base64,{encoded}"),
            )])
        })
    }

    pub(crate) fn convert(&self, image: &Image) -> Result<Vec<(String, String)>, BoxError> {
        (self.0)(image)
    }
}

impl Default for ImageConverter {
    fn default() -> Self {
        ImageConverter::data_uri()
    }
}

impl fmt::Debug for ImageConverter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImageConverter(..)")
    }
}

/// The file extension for an image's content type, e.g. `png` for `image/png`.
pub fn image_filename_extension(image: &Image) -> Option<&str> {
    image
        .content_type
        .as_deref()
        .and_then(|content_type| content_type.split(['/', '\\']).nth(1))
}
