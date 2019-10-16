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
pub struct MRPayload<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub source_branch: &'a str,
    pub target_branch: &'a str,
    pub labels: &'a str,
    pub remove_source_branch: bool,
    pub squash: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub group: Option<String>,
    pub user: Option<String>,
    pub mr_labels: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Clone)]
pub struct MRRequest<'a> {
    pub access_token: &'a str,
    pub project: &'a ProjectResponse,
    pub title: &'a str,
    pub description: &'a str,
    pub source_branch: &'a str,
    pub target_branch: &'a str,
}
