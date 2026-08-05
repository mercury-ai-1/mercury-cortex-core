use mercury_cortex_core::client::CoreClient;
use mercury_cortex_core::db::DB_FILENAME;
use mercury_cortex_core::db::initialize;
use tempfile::TempDir;

#[tokio::test]
async fn open_with_data_dir_connects_lazily_and_uses_the_data_dir() {
    let tmp = TempDir::new().unwrap();
    let client = CoreClient::open_with_data_dir(tmp.path().to_path_buf()).unwrap();

    assert_eq!(
        client.database().list_resettable_tables().await.unwrap(),
        Vec::<String>::new(),
        "fresh DB has no schema tables"
    );
    assert!(
        tmp.path().join(DB_FILENAME).exists(),
        "lazy connect must create the DB inside the configured data dir"
    );
    assert!(client.profile().get().await.unwrap().is_none());
}

#[tokio::test]
async fn from_connection_wraps_an_existing_db() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(DB_FILENAME);
    let db = initialize(&db_path).await.unwrap();

    let client = CoreClient::from_connection(db, tmp.path().to_path_buf()).unwrap();
    assert!(client.profile().get().await.unwrap().is_none());
    assert!(
        client
            .database()
            .list_resettable_tables()
            .await
            .unwrap()
            .is_empty()
    );
}

#[test]
fn paths_resolve_against_home_without_connecting() {
    let paths = CoreClient::paths().unwrap();
    assert_eq!(paths.db_path, paths.data_dir.join(DB_FILENAME));
}
