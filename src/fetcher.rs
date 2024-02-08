use std::{any::type_name, future::Future, sync::Arc};

use chrono::{DateTime, Utc};
use log::{error, info};
use octocrab::{models, params, Octocrab};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

#[derive(Debug)]
pub struct Fetcher {
    repo: (String, String), // (owner, repo_name)
    octocrab: Arc<Octocrab>,
    start_time: DateTime<Utc>,
}

impl Fetcher {
    /// Create a new fetcher instance for the given repository.
    pub fn new(
        octocrab: Octocrab,
        repo: &str,
        start_time: impl Into<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Self, &'static str> {
        info!("fetcher init with repo '{}'", repo);
        Ok(Self {
            repo: repo
                .split_once('/')
                .map(|(owner, repo_name)| (owner.to_string(), repo_name.to_string()))
                .ok_or("invalid repo name, should be 'owner/repo_name'")?,
            octocrab: Arc::new(octocrab),
            start_time: start_time.into(),
        })
    }

    /// Fetch all the issues (including PRs) from the repository.
    pub fn fetch_issues(&self) -> UnboundedReceiver<Vec<models::issues::Issue>> {
        self.fetch(|octocrab, owner, repo_name, start_time| async move {
            match octocrab
                .issues(owner.clone(), repo_name.clone())
                .list()
                .state(params::State::All)
                .since(start_time)
                .send()
                .await
            {
                Ok(res) => res.items,
                Err(err) => {
                    error!(
                        "failed to fetch issues from {}/{}: {}",
                        owner, repo_name, err
                    );
                    vec![]
                }
            }
        })
    }

    /// Fetch all the comments of the issues from the repository.
    pub fn fetch_issue_comments(
        &self,
        issue_ids: Vec<u64>,
    ) -> UnboundedReceiver<Vec<models::issues::Comment>> {
        self.fetch(|octocrab, owner, repo_name, start_time| async move {
            let mut comments = Vec::new();
            for issue_id in issue_ids {
                match octocrab
                    .issues(owner.clone(), repo_name.clone())
                    .list_comments(issue_id)
                    .since(start_time)
                    .send()
                    .await
                {
                    Ok(res) => comments.extend(res.items),
                    Err(err) => {
                        error!(
                            "failed to fetch issue comments from {}/{}#{}: {}",
                            owner, repo_name, issue_id, err
                        );
                    }
                }
            }
            comments
        })
    }

    /// Fetch all the comments of the pull requests from the repository.
    pub fn fetch_pull_request_comments(&self) -> UnboundedReceiver<Vec<models::pulls::Comment>> {
        self.fetch(move |octocrab, owner, repo_name, start_time| async move {
            match octocrab
                .pulls(owner.clone(), repo_name.clone())
                .list_comments(None)
                .since(start_time)
                .send()
                .await
            {
                Ok(res) => res.items,
                Err(err) => {
                    error!(
                        "failed to fetch pull request comments from {}/{}: {}",
                        owner, repo_name, err
                    );
                    vec![]
                }
            }
        })
    }

    /// Fetch all the reviews of the pull requests from the repository.
    pub fn fetch_pull_request_reviews(
        &self,
        pull_request_ids: Vec<u64>,
    ) -> UnboundedReceiver<Vec<models::pulls::Review>> {
        self.fetch(move |octocrab, owner, repo_name, _| async move {
            let mut reviews = Vec::new();
            for pull_request_id in pull_request_ids {
                match octocrab
                    .pulls(owner.clone(), repo_name.clone())
                    .list_reviews(pull_request_id)
                    .send()
                    .await
                {
                    Ok(res) => reviews.extend(res.items),
                    Err(err) => {
                        error!(
                            "failed to fetch pull request reviews from {}/{}#{}: {}",
                            owner, repo_name, pull_request_id, err
                        );
                    }
                }
            }
            reviews
        })
    }

    fn fetch<T, F, R>(&self, fetch_fn: F) -> UnboundedReceiver<Vec<T>>
    where
        T: 'static + Send,
        F: 'static + Send + FnOnce(Arc<Octocrab>, String, String, DateTime<Utc>) -> R,
        R: Send + Future<Output = Vec<T>>,
    {
        let (owner, repo_name) = (self.repo.0.clone(), self.repo.1.clone());
        info!(
            "fetching {} data from '{}/{}'",
            type_name::<T>(),
            self.repo.0,
            self.repo.1,
        );
        let (tx, rx) = unbounded_channel();
        let octocrab = self.octocrab.clone();
        let start_time = self.start_time.clone();
        tokio::spawn(
            async move { tx.send(fetch_fn(octocrab, owner, repo_name, start_time).await) },
        );
        rx
    }
}
