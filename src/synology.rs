use reqwest::{Client, ClientBuilder, Error as ReqwestError};
use serde::{Deserialize, Serialize};
use log::{info, error, debug};
use std::fmt;
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr};

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

impl SynologyError {
    pub fn get_error_description(&self) -> &str {
        match self.code {
            100 => "Unknown error.",
            101 => "No parameter of API, method or version.",
            102 => "The requested API does not exist.",
            103 => "The requested method does not exist.",
            104 => "The requested version does not support the functionality.",
            105 => "The logged in session does not have permission.",
            106 => "Session timeout.",
            107 => "Session interrupted by duplicated login.",
            108 => "Failed to upload the file.",
            109 => "The network connection is unstable or the system is busy.",
            110 => "The network connection is unstable or the system is busy.",
            111 => "The network connection is unstable or the system is busy.",
            112 => "Preserve for other purpose.",
            113 => "Preserve for other purpose.",
            114 => "Lost parameters for this API.",
            115 => "Not allowed to upload a file.",
            116 => "Not allowed to perform for a demo site.",
            117 => "The network connection is unstable or the system is busy.",
            118 => "The network connection is unstable or the system is busy.",
            119 => "Invalid session.",
            150 => "Request source IP does not match the login IP.",
            _ => {
                if self.code >= 120 && self.code <= 149 {
                    "Preserve for other purpose."
                } else {
                    "Unknown error code."
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum SynologyClientError {
    /// Error from the reqwest library
    Reqwest(ReqwestError),
    /// Error from the Synology API
    Synology(SynologyError),
    /// Generic error with a message
    Generic(String),
    /// Login failed
    LoginFailed,
}

impl fmt::Display for SynologyClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SynologyClientError::Reqwest(err) => write!(f, "HTTP error: {}", err),
            SynologyClientError::Synology(err) => write!(f, "Synology API error: {} - {}", err.code, err.get_error_description()),
            SynologyClientError::Generic(msg) => write!(f, "{}", msg),
            SynologyClientError::LoginFailed => write!(f, "Login failed"),
        }
    }
}

impl Error for SynologyClientError {}

impl From<ReqwestError> for SynologyClientError {
    fn from(err: ReqwestError) -> Self {
        SynologyClientError::Reqwest(err)
    }
}

impl From<SynologyError> for SynologyClientError {
    fn from(err: SynologyError) -> Self {
        SynologyClientError::Synology(err)
    }
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

impl From<FileListData> for Vec<FileInfo> {
    fn from(data: FileListData) -> Self {
        data.files
    }
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
    #[serde(rename = "service_status", default)]
    pub service_status: bool,

    // Alternative field names that might be in the response
    #[serde(rename = "enable_ssh", alias = "enable", skip_serializing, default)]
    pub enable_ssh: Option<bool>,

    #[serde(rename = "status", alias = "ssh_status", skip_serializing, default)]
    pub status: Option<bool>,
}

impl From<ServiceStatusData> for bool {
    fn from(data: ServiceStatusData) -> Self {
        data.service_status 
            || data.enable_ssh.unwrap_or(false) 
            || data.status.unwrap_or(false)
    }
}

pub struct SynologyClient {
    client: Client,
    base_url: String,
    username: String,
    password: String,
    sid: Option<String>,
    force_ipv4: bool,
}

impl SynologyClient {
    pub fn new(base_url: &str, username: &str, password: &str, force_ipv4: bool) -> Self {
        // Create a client with cookie storage disabled and optionally force IPv4
        let mut client_builder = ClientBuilder::new()
            .cookie_store(false);

        // If force_ipv4 is true, configure the client to use IPv4 only
        if force_ipv4 {
            let ipv4_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)); // 0.0.0.0
            client_builder = client_builder.local_address(ipv4_addr);
            debug!("Forcing IPv4 for Synology API requests");
        }

        let client = client_builder
            .build()
            .expect("Failed to build reqwest client");

        SynologyClient {
            client,
            base_url: base_url.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            sid: None,
            force_ipv4,
        }
    }

    pub(crate) async fn logout(&mut self) -> Result<(), SynologyClientError> {
        if self.sid.is_none() {
            debug!("Not logged in, no need to logout");
            return Ok(());
        }

        let url = self.get_url(AUTH_ENDPOINT);
        info!("Logging out from Synology NAS...");

        let params = [
                ("api", "SYNO.API.Auth"),
                ("version", "3"),
                ("method", "logout"),
                ("_sid", self.sid.as_ref().unwrap()),
            ];

        let builder = self.client
            .get(&url)
            .query(&params);
        debug!("Synology request {:?}", builder);

        // Log the equivalent curl command
        let curl_cmd = self.to_curl_command(&url, &params, &[]);
        debug!("Equivalent curl command: {}", curl_cmd);

        let response = builder
            .send()
            .await?
            .error_for_status()?;

        debug!("Logout response: {:?}", response);

        // Clear the session ID
        self.sid = None;
        info!("Successfully logged out from Synology NAS");

        Ok(())
    }

    pub(crate) async fn login(&mut self) -> Result<(), SynologyClientError> {
        let url = self.get_url(AUTH_ENDPOINT);

        info!("Logging in to Synology NAS...");

        let params = [
                ("api", "SYNO.API.Auth"),
                ("version", "3"),
                ("method", "login"),
                ("account", &self.username),
                ("passwd", &self.password),
            ];

        let builder = self.client
            .get(&url)
            .query(&params);

        debug!("Synology request {:?}", builder);

        // Log the equivalent curl command
        let curl_cmd = self.to_curl_command(&url, &params, &["passwd"]);
        debug!("Equivalent curl command: {}", curl_cmd);

        let response = builder
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

        self.handle_error_response(auth_response.error, "Login failed")
    }

    fn get_url(&mut self, endpoint: &str) -> String {
        format!("{}/webapi{}", self.base_url, endpoint)
    }

    // Helper method to log requests with optional parameter masking
    fn log_request(&self, url: &str, params: &[(&str, &str)], mask_params: &[&str]) {
        // Create a copy of params with masked values for sensitive parameters
        let masked_params: Vec<(&str, &str)> = params.iter()
            .map(|(key, value)| {
                if mask_params.contains(key) {
                    (*key, "********")
                } else {
                    (*key, *value)
                }
            })
            .collect();

        debug!("Sending Synology API request to: {} with params: {:?}", url, masked_params);

        // Log the equivalent curl command
        let curl_cmd = self.to_curl_command(url, params, mask_params);
        debug!("Equivalent curl command: {}", curl_cmd);
    }

    // Helper method to convert a request to its equivalent curl command
    fn to_curl_command(&self, url: &str, params: &[(&str, &str)], mask_params: &[&str]) -> String {
        // Start with the base curl command
        let mut curl_cmd = format!("curl -X GET");

        // Add the URL with query parameters
        let mut first_param = true;
        let mut full_url = url.to_string();

        for (key, value) in params {
            let param_value = if mask_params.contains(key) {
                "********"
            } else {
                value
            };

            if first_param {
                full_url.push_str(&format!("?{}={}", key, param_value));
                first_param = false;
            } else {
                full_url.push_str(&format!("&{}={}", key, param_value));
            }
        }

        // Add the URL to the curl command (with proper escaping)
        curl_cmd.push_str(&format!(" '{}'", full_url.replace("'", "\\'")));

        curl_cmd
    }

    async fn ensure_login(&mut self) -> Result<bool, SynologyClientError> {
        if self.sid.is_none() {
            debug!("Not logged in. Attempting automatic login...");
            self.login().await?;
        }
        Ok(self.sid.is_some())
    }

    // Generic method to handle API requests
    async fn api_request<T, R>(
        &mut self, 
        endpoint: &str, 
        api: &str, 
        version: &str, 
        method: &str, 
        additional_params: Vec<(&str, &str)>,
        operation_name: &str
    ) -> Result<R, SynologyClientError> 
    where 
        T: for<'de> Deserialize<'de>,
        R: From<T>
    {
        if !self.ensure_login().await? {
            error!("Login attempt failed. Cannot {}.", operation_name);
            return Err(SynologyClientError::LoginFailed);
        }

        let url = self.get_url(endpoint);

        // Build base query parameters
        let mut params = vec![
            ("api", api),
            ("version", version),
            ("method", method),
            ("_sid", self.sid.as_ref().unwrap()),
        ];
        params.extend(additional_params);

        // Log the request using the helper method (no sensitive params to mask)
        let builder = self.client
            .get(&url)
            .query(&params);
        debug!("Synology request {:?}", builder);

        // Log the equivalent curl command
        let curl_cmd = self.to_curl_command(&url, &params, &[]);
        debug!("Equivalent curl command: {}", curl_cmd);

        // Send request
        let response = builder
            .send()
            .await?
            .error_for_status()?;

        let body_text = response.text().await?;
        debug!("Response body: {}", body_text);

        // Parse response
        let api_response: SynologyResponse<T> = match serde_json::from_str(&body_text) {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to parse response: {}", e);
                return Err(SynologyClientError::Generic(format!("JSON parsing error: {}", e)));
            }
        };

        if api_response.success {
            if let Some(data) = api_response.data {
                return Ok(data.into());
            }
        }

        self.handle_error_response(api_response.error, &format!("{} failed", operation_name))
    }

    // Helper method to handle error responses
    fn handle_error_response<T>(&self, error: Option<SynologyError>, operation: &str) -> Result<T, SynologyClientError> {
        if let Some(error) = error {
            let error_msg = format!("{} with error code: {} - {}", operation, error.code, error.get_error_description());
            error!("{}", error_msg);
            Err(SynologyClientError::Synology(error))
        } else {
            let error_msg = format!("{} with unknown error", operation);
            error!("{}", error_msg);
            Err(SynologyClientError::Generic(error_msg.to_string()))
        }
    }

    pub async fn list_files(&mut self, folder_path: &str) -> Result<Vec<FileInfo>, SynologyClientError> {
        info!("Listing files in folder: {}", folder_path);

        // Explicitly login before the request
        self.login().await?;

        // Use a match to ensure logout happens even if there's an error
        let result = self.api_request::<FileListData, Vec<FileInfo>>(
            FILESTATION_ENDPOINT,
            "SYNO.FileStation.List",
            "2",
            "list",
            vec![("folder_path", folder_path)],
            &format!("list files in {}", folder_path)
        ).await;

        // Always logout after the request
        if let Err(e) = self.logout().await {
            error!("Failed to logout after list_files: {}", e);
        }

        result
    }

    pub async fn get_ssh_status(&mut self) -> Result<bool, SynologyClientError> {
        // Explicitly login before the request
        self.login().await?;

        // Use a match to ensure logout happens even if there's an error
        let api_result = self.api_request::<ServiceStatusData, bool>(
            TERMINAL_ENDPOINT,
            "SYNO.Core.Terminal",
            "1",
            "get",
            vec![],
            "get SSH service status"
        ).await;

        // Always logout after the request
        if let Err(e) = self.logout().await {
            error!("Failed to logout after get_ssh_status: {}", e);
        }

        // Process the result
        let result = api_result?;
        info!("SSH service status: {}", if result { "enabled" } else { "disabled" });
        Ok(result)
    }

    pub async fn toggle_ssh(&mut self, enable: bool) -> Result<(), SynologyClientError> {
        info!("{} SSH service...", if enable { "Enabling" } else { "Disabling" });

        // Explicitly login before the request
        self.login().await?;

        let enable_ssh_new_state = if enable { "true" } else { "false" };

        // Use a match to ensure logout happens even if there's an error
        let api_result = self.api_request::<(), ()>(
            TERMINAL_ENDPOINT,
            "SYNO.Core.Terminal",
            "1",
            "set",
            vec![("enable_ssh", enable_ssh_new_state)],
            &format!("{} SSH service", if enable { "enable" } else { "disable" })
        ).await;

        // Always logout after the request
        if let Err(e) = self.logout().await {
            error!("Failed to logout after toggle_ssh: {}", e);
        }

        // Process the result
        let result = api_result?;
        info!("Successfully {} SSH service", if enable { "enabled" } else { "disabled" });
        Ok(result)
    }
}
