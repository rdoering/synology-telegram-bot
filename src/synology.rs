use reqwest::{Client, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use log::{info, error, debug};

// Synology API endpoints
const AUTH_ENDPOINT: &str = "/entry.cgi";
const FILESTATION_ENDPOINT: &str = "/entry.cgi";
const TERMINAL_ENDPOINT: &str = "/entry.cgi";

#[derive(Debug, Serialize, Deserialize)]
pub struct SynologyResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<SynologyError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SynologyError {
    pub code: i32,
    #[serde(rename = "errors")]
    pub error_details: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthData {
    pub sid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileListData {
    pub files: Vec<FileInfo>,
    pub total: i32,
    pub offset: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub isdir: bool,
    pub size: Option<u64>,
    pub time: Option<FileTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileTime {
    pub ctime: u64,
    pub mtime: u64,
    pub atime: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceStatusData {
    pub service_status: bool,
}

pub struct SynologyClient {
    client: Client,
    base_url: String,
    username: String,
    password: String,
    sid: Option<String>,
}

impl SynologyClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        SynologyClient {
            client: Client::new(),
            base_url: base_url.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            sid: None,
        }
    }

    pub(crate) async fn login(&mut self) -> Result<(), ReqwestError> {
        let url = self.get_url(AUTH_ENDPOINT);

        info!("Logging in to Synology NAS...");

        let response = self.client
            .get(&url)
            .query(&[
                ("api", "SYNO.API.Auth"),
                ("version", "3"),
                ("method", "login"),
                ("account", &self.username),
                ("passwd", &self.password),
            ])
            .send()
            .await?
            .error_for_status()?;

        debug!("parsing json response {:?}", response);

        let auth_response: SynologyResponse<AuthData> = response.json().await?;

        if auth_response.success {
            if let Some(data) = auth_response.data {
                self.sid = Some(data.sid);
                info!("Successfully logged in to Synology NAS");
                return Ok(());
            }
        }

        if let Some(error) = auth_response.error {
            error!("Login failed with error code: {}", error.code);
        } else {
            error!("Login failed with unknown error");
        }

        Ok(())
    }

    fn get_url(&mut self, endpoint: &str) -> String {
        format!("{}/webapi{}", self.base_url, endpoint)
    }

    async fn ensure_login(&mut self) -> Result<bool, ReqwestError> {
        if self.sid.is_none() {
            debug!("Not logged in. Attempting automatic login...");
            self.login().await?;
        }
        Ok(self.sid.is_some())
    }

    pub async fn list_files(&mut self, folder_path: &str) -> Result<Vec<FileInfo>, ReqwestError> {
        if !self.ensure_login().await? {
            error!("Login attempt failed. Cannot list files.");
            return Ok(Vec::new());
        }

        let url = self.get_url(FILESTATION_ENDPOINT);

        info!("Listing files in folder: {}", folder_path);

        let response = self.client
            .get(&url)
            .query(&[
                ("api", "SYNO.FileStation.List"),
                ("version", "2"),
                ("method", "list"),
                ("folder_path", folder_path),
                ("_sid", self.sid.as_ref().unwrap()),
            ])
            .send()
            .await?;

        let file_list_response: SynologyResponse<FileListData> = response.json().await?;

        if file_list_response.success {
            if let Some(data) = file_list_response.data {
                return Ok(data.files);
            }
        }

        if let Some(error) = file_list_response.error {
            error!("List files failed with error code: {}", error.code);
        } else {
            error!("List files failed with unknown error");
        }

        Ok(Vec::new())
    }

    pub async fn get_ssh_status(&mut self) -> Result<bool, ReqwestError> {
        if !self.ensure_login().await? {
            error!("Login attempt failed. Cannot get SSH status.");
            return Ok(false);
        }

        let url = format!("{}{}", self.base_url, TERMINAL_ENDPOINT);

        info!("Getting SSH service status...");

        let response = self.client
            .get(&url)
            .query(&[
                ("api", "SYNO.Core.Terminal"),
                ("version", "1"),
                ("method", "get"),
                ("_sid", self.sid.as_ref().unwrap()),
            ])
            .send()
            .await?;

        let status_response: SynologyResponse<ServiceStatusData> = response.json().await?;

        if status_response.success {
            if let Some(data) = status_response.data {
                info!("SSH service status: {}", if data.service_status { "enabled" } else { "disabled" });
                return Ok(data.service_status);
            }
        }

        if let Some(error) = status_response.error {
            error!("Get SSH status failed with error code: {}", error.code);
        } else {
            error!("Get SSH status failed with unknown error");
        }

        Ok(false)
    }

    pub async fn toggle_ssh(&mut self, enable: bool) -> Result<(), ReqwestError> {
        if !self.ensure_login().await? {
            error!("Login attempt failed. Cannot toggle SSH.");
            return Ok(());
        }

        let url = self.get_url(TERMINAL_ENDPOINT);

        info!("{} SSH service...", if enable { "Enabling" } else { "Disabling" });

        let response = self.client
            .get(&url)
            .query(&[
                ("api", "SYNO.Core.Terminal"),
                ("version", "1"),
                ("method", "set"),
                ("service_status", if enable { "true" } else { "false" }),
                ("_sid", self.sid.as_ref().unwrap()),
            ])
            .send()
            .await?;

        let toggle_response: SynologyResponse<()> = response.json().await?;

        if toggle_response.success {
            info!("Successfully {} SSH service", if enable { "enabled" } else { "disabled" });
        } else if let Some(error) = toggle_response.error {
            error!("Toggle SSH failed with error code: {}", error.code);
        } else {
            error!("Toggle SSH failed with unknown error");
        }

        Ok(())
    }
}
