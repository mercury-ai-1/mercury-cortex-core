//! Service layer — business logic for profiles, projects, and file data.
pub mod error;
pub mod file_data;
pub mod graph;
pub mod profile;
pub mod project;
pub mod scaffold;

pub use error::ServiceError;
