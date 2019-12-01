use serde_json::Number;

#[derive(Serialize, Deserialize, Debug)]
pub struct GroupResponse {
    pub id: Number,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProjectResponse {
    pub id: Number,
    pub name: String,
    pub ssh_url_to_repo: String,
    pub http_url_to_repo: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MergeRequestResponse {
    pub id: Number,
    pub title: String,
    pub author: Author,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Author {
    pub id: Number,
    pub name: String,
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct MRResponse {
    pub web_url: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MRPayload {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
    pub labels: String,
    pub remove_source_branch: bool,
    pub squash: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub group: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub apikey: Option<String>,
    pub ssh_key_file: Option<String>,
    pub ssh_passphrase: Option<String>,
    pub mr_labels: Option<Vec<String>>,
    #[serde(default = "default_host")]
    pub host: String,
}

fn default_host() -> String {
    "https://gitlab.com".to_string()
}

#[derive(Debug, Serialize, Clone)]
pub struct MRRequest<'a> {
    pub access_token: String,
    pub project: &'a ProjectResponse,
    pub title: String,
    pub description: String,
    pub source_branch: String,
    pub target_branch: String,
}
