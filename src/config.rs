use std::{error::Error, fs, path::Path};

use serde::Deserialize;

const FEISHU_BOT_WEBHOOK_URL_ENV: &str = "GH_OVERSEER_FEISHU_BOT_WEBHOOK_URL";
const GITHUB_PERSONAL_TOKEN_ENV: &str = "GH_OVERSEER_GITHUB_PERSONAL_TOKEN";

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    access: Access,
    review: Review,
}

#[derive(Debug, Clone, Deserialize)]
struct Access {
    // TODO: support send the stats result to the Feishu/Lark bot.
    feishu_bot_webhook_url: String,
    github_personal_token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Review {
    users: Vec<String>,
    repos: Vec<String>,
    lgtm_comments: Vec<String>,
}

impl Config {
    pub fn load<P: AsRef<Path>>(config_path: P) -> Result<Self, Box<dyn Error>> {
        toml::from_str(
            fs::read_to_string(config_path)
                .expect("failed to load config file")
                .as_str(),
        )
        .map_err(|e| e.into())
    }

    pub fn feishu_bot_webhook_url(&self) -> String {
        if let Ok(url) = std::env::var(FEISHU_BOT_WEBHOOK_URL_ENV) {
            url
        } else {
            self.access.feishu_bot_webhook_url.clone()
        }
    }

    pub fn github_personal_token(&self) -> String {
        if let Ok(token) = std::env::var(GITHUB_PERSONAL_TOKEN_ENV) {
            token
        } else {
            self.access.github_personal_token.clone()
        }
    }

    pub fn review_users(&self) -> Vec<String> {
        self.review.users.clone()
    }

    pub fn review_repos(&self) -> Vec<String> {
        self.review.repos.clone()
    }

    /// Get the comments that are considered as a LGTM approval.
    pub fn review_lgtm_comments(&self) -> Vec<String> {
        self.review.lgtm_comments.clone()
    }
}
