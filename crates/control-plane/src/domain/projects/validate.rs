//! Validation en liste blanche des entrees d'un projet. Les regles sont
//! dans `shared::validate`, communes avec l'agent (memes valeurs acceptees
//! des deux cotes) ; ici, leur application aux types de l'API.

use api_types::projects::{Build, GitSource, ImageSource, Source};

pub use shared::validate::{
    Invalid, MAX_BRANCH, MAX_IMAGE, MAX_NAME, MAX_PATH, branch, compose_service, image, name, path,
    project_name, repository,
};

pub fn build(value: &Build) -> Result<(), Invalid> {
    match value {
        Build::Railpack => Ok(()),
        Build::Dockerfile { path: p } => path(p),
        Build::Compose { path: p, service } => {
            path(p)?;
            compose_service(service)
        }
    }
}

/// Source validee : le depot Git est ramene a `proprietaire/nom`.
pub fn source(value: Source) -> Result<Source, Invalid> {
    match value {
        Source::Git(GitSource {
            repository: repo,
            branch: b,
        }) => {
            branch(&b)?;
            Ok(Source::Git(GitSource {
                repository: repository(&repo)?,
                branch: b,
            }))
        }
        Source::Image(ImageSource { image: i }) => {
            image(&i)?;
            Ok(Source::Image(ImageSource { image: i }))
        }
    }
}
