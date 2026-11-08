use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use napi::{Result, bindgen_prelude::Function, threadsafe_function::ThreadsafeFunctionCallMode};
use napi_derive::napi;
use quadrant_host::{QuadrantHost, QuadrantHostOptions};
use serde_json::Value;

#[napi(object)]
pub struct QuadrantHostInit {
    pub data_dir: String,
    pub mc_folder: Option<String>,
    pub api_base_url: Option<String>,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub quadrant_api_key: String,
    pub config_store_name: Option<String>,
    pub keyring_service_name: Option<String>,
    pub app_version: Option<String>,
    pub os_name: Option<String>,
    pub user_agent: Option<String>,
}

#[napi]
pub struct QuadrantHostAddon {
    host: QuadrantHost,
    event_forwarder: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

#[napi]
impl QuadrantHostAddon {
    #[napi(constructor)]
    pub fn new(options: QuadrantHostInit) -> Result<Self> {
        let mut host_options = QuadrantHostOptions::new(
            PathBuf::from(options.data_dir),
            options.oauth_client_id,
            options.oauth_client_secret,
            options.quadrant_api_key,
        );
        host_options.mc_folder = options.mc_folder.map(PathBuf::from);
        host_options.api_base_url = options.api_base_url;
        if let Some(value) = options.config_store_name {
            host_options.config_store_name = value;
        }
        if let Some(value) = options.keyring_service_name {
            host_options.keyring_service_name = value;
        }
        if let Some(value) = options.app_version {
            host_options.app_version = value;
        }
        if let Some(value) = options.os_name {
            host_options.os_name = value;
        }
        if let Some(value) = options.user_agent {
            host_options.user_agent = value;
        }

        Ok(Self {
            host: QuadrantHost::new(host_options)
                .map_err(|error| napi::Error::from_reason(error.to_string()))?,
            event_forwarder: Arc::new(Mutex::new(None)),
        })
    }

    #[napi]
    pub async fn start_background_workers(&self) -> Result<()> {
        self.host
            .start_background_workers()
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn stop_background_workers(&self) -> Result<()> {
        self.host
            .stop_background_workers()
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn shutdown(&self) -> Result<()> {
        self.host.shutdown().await.map_err(to_napi_error)?;
        self.clear_event_forwarder();
        Ok(())
    }

    #[napi]
    pub fn on_event(&self, callback: Function<'_, String, ()>) -> Result<()> {
        self.clear_event_forwarder();

        let tsfn = callback.build_threadsafe_function().build()?;
        let mut receiver = self.host.subscribe_events();
        let handle = napi::bindgen_prelude::within_runtime_if_available(|| {
            tokio::spawn(async move {
                loop {
                    match receiver.recv().await {
                        Ok(event) => {
                            if let Ok(payload) = serde_json::to_string(&event) {
                                let _ = tsfn.call(payload, ThreadsafeFunctionCallMode::NonBlocking);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            })
        });

        *self
            .event_forwarder
            .lock()
            .map_err(|_| napi::Error::from_reason("event forwarder is busy"))? = Some(handle);
        Ok(())
    }

    #[napi]
    pub async fn invoke(&self, command: String, payload: Option<Value>) -> Result<Value> {
        self.host
            .invoke(&command, payload.unwrap_or(Value::Null))
            .await
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn init_config(&self) -> Result<()> {
        self.host.init_config().map_err(to_napi_error)
    }

    #[napi]
    pub async fn get_minecraft_folder(&self) -> Result<String> {
        Ok(self
            .host
            .get_minecraft_folder()
            .map_err(to_napi_error)?
            .to_string_lossy()
            .to_string())
    }

    #[napi]
    pub async fn get_modpacks(&self, hide_free: Option<bool>) -> Result<Value> {
        self.host
            .get_modpacks(hide_free.unwrap_or(false))
            .await
            .and_then(|value| serde_json::to_value(value).map_err(Into::into))
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn get_account_info(&self) -> Result<Value> {
        self.host
            .get_account_info()
            .await
            .and_then(|value| serde_json::to_value(value).map_err(Into::into))
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn get_news(&self) -> Result<Value> {
        self.host
            .get_news()
            .await
            .and_then(|value| serde_json::to_value(value).map_err(Into::into))
            .map_err(to_napi_error)
    }

    #[napi]
    pub async fn install_mod(&self, args: Value) -> Result<()> {
        self.host
            .invoke("install_mod", args)
            .await
            .map_err(to_napi_error)
            .map(|_| ())
    }

    #[napi]
    pub async fn sync_modpack(&self, modpack: Value, overwrite: bool) -> Result<()> {
        self.host
            .invoke(
                "sync_modpack",
                serde_json::json!({
                    "modpack": modpack,
                    "overwrite": overwrite,
                }),
            )
            .await
            .map_err(to_napi_error)
            .map(|_| ())
    }
}

impl QuadrantHostAddon {
    fn clear_event_forwarder(&self) {
        if let Some(handle) = self
            .event_forwarder
            .lock()
            .expect("event forwarder lock poisoned")
            .take()
        {
            handle.abort();
        }
    }
}

fn to_napi_error(error: impl Into<anyhow::Error>) -> napi::Error {
    napi::Error::from_reason(quadrant_host::user_facing(error.into()).to_string())
}
