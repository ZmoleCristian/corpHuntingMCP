use reqwest::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::AtsError;

pub async fn get_json<T: DeserializeOwned>(
    client: &Client,
    platform: &'static str,
    tenant: &str,
    url: &str,
) -> Result<T, AtsError> {
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(source) => {
            return Err(AtsError::Http {
                platform,
                tenant: tenant.to_string(),
                source,
            });
        }
    };
    let body = body_of(response, platform, tenant, url).await?;
    parse(&body, platform, tenant)
}

pub async fn post_json<B: Serialize, T: DeserializeOwned>(
    client: &Client,
    platform: &'static str,
    tenant: &str,
    url: &str,
    payload: &B,
) -> Result<T, AtsError> {
    let response = match client.post(url).json(payload).send().await {
        Ok(response) => response,
        Err(source) => {
            return Err(AtsError::Http {
                platform,
                tenant: tenant.to_string(),
                source,
            });
        }
    };
    let body = body_of(response, platform, tenant, url).await?;
    parse(&body, platform, tenant)
}

pub async fn get_text(
    client: &Client,
    platform: &'static str,
    tenant: &str,
    url: &str,
) -> Result<String, AtsError> {
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(source) => {
            return Err(AtsError::Http {
                platform,
                tenant: tenant.to_string(),
                source,
            });
        }
    };
    body_of(response, platform, tenant, url).await
}

async fn body_of(
    response: reqwest::Response,
    platform: &'static str,
    tenant: &str,
    url: &str,
) -> Result<String, AtsError> {
    let status = response.status();
    if !status.is_success() {
        return Err(AtsError::Status {
            platform,
            tenant: tenant.to_string(),
            status: status.as_u16(),
            url: url.to_string(),
        });
    }
    match response.text().await {
        Ok(body) => Ok(body),
        Err(source) => Err(AtsError::Decode {
            platform,
            tenant: tenant.to_string(),
            source,
        }),
    }
}

fn parse<T: DeserializeOwned>(
    body: &str,
    platform: &'static str,
    tenant: &str,
) -> Result<T, AtsError> {
    match serde_json::from_str::<T>(body) {
        Ok(value) => Ok(value),
        Err(source) => Err(AtsError::Parse {
            platform,
            tenant: tenant.to_string(),
            source,
        }),
    }
}
