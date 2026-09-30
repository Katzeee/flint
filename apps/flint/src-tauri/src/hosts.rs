//! JSON presentation of local host inspection shared by CLI and desktop callers.
use base64::Engine;
use flint_hosts::{HostInfo, WindowPreview};
use serde_json::{json, Value};

pub fn host_info_json(info: HostInfo) -> Value {
    let mut value = json!(info.candidate);
    value["window"] = json!(info.window);
    if let Some(preview) = info.preview {
        value["preview"] = match preview {
            WindowPreview::Png(png) => json!({
                "image": format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                ),
                "unavailable_reason": null,
            }),
            WindowPreview::Unavailable(reason) => {
                json!({"image": null, "unavailable_reason": reason})
            }
        };
    }
    value
}
