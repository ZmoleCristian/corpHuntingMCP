use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::HandleError;
use crate::model::Ats;

const SEPARATOR: char = '\t';
const FIELDS: usize = 3;

#[derive(Debug, Clone)]
pub struct Handle {
    pub ats: Ats,
    pub tenant: String,
    pub job: String,
}

pub fn encode(ats: Ats, tenant: &str, job: &str) -> String {
    let raw = format!(
        "{}{}{}{}{}",
        ats.label(),
        SEPARATOR,
        tenant,
        SEPARATOR,
        job
    );
    URL_SAFE_NO_PAD.encode(raw)
}

pub fn decode(handle: &str) -> Result<Handle, HandleError> {
    let bytes = match URL_SAFE_NO_PAD.decode(handle) {
        Ok(bytes) => bytes,
        Err(source) => {
            return Err(HandleError::Encoding {
                handle: handle.to_string(),
                source,
            });
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(source) => {
            return Err(HandleError::Utf8 {
                handle: handle.to_string(),
                source,
            });
        }
    };
    let mut parts = text.splitn(FIELDS, SEPARATOR);
    let Some(platform) = parts.next() else {
        return Err(HandleError::Shape {
            handle: handle.to_string(),
        });
    };
    let Some(tenant) = parts.next() else {
        return Err(HandleError::Shape {
            handle: handle.to_string(),
        });
    };
    let Some(job) = parts.next() else {
        return Err(HandleError::Shape {
            handle: handle.to_string(),
        });
    };
    let ats = match Ats::parse(platform) {
        Ok(ats) => ats,
        Err(source) => {
            return Err(HandleError::Platform {
                handle: handle.to_string(),
                source,
            });
        }
    };
    Ok(Handle {
        ats,
        tenant: tenant.to_string(),
        job: job.to_string(),
    })
}
