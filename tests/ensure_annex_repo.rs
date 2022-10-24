use gamdam::cmd::LoggedCommand;
use gamdam::ensure_annex_repo;
use tempfile::tempdir;

#[tokio::test]
async fn test_new_repo() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    ensure_annex_repo(tmp_path).await.unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
}

#[tokio::test]
async fn test_git_repo() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    LoggedCommand::new("git", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    assert!(tmp_path.join(".git").exists());
    ensure_annex_repo(tmp_path).await.unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
}

#[tokio::test]
async fn test_git_annex_repo() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    LoggedCommand::new("git", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    LoggedCommand::new("git-annex", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
    ensure_annex_repo(tmp_path).await.unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
}

#[tokio::test]
async fn test_multidir() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    let repo = tmp_path.join("foo").join("bar").join("baz");
    ensure_annex_repo(&repo).await.unwrap();
    assert!(!tmp_path.join(".git").exists());
    assert!(repo.join(".git").exists());
    assert!(repo.join(".git").join("annex").exists());
}

#[tokio::test]
async fn git_subdir() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    LoggedCommand::new("git", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    assert!(tmp_path.join(".git").exists());
    let repo = tmp_path.join("foo").join("bar").join("baz");
    ensure_annex_repo(&repo).await.unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
    assert!(!repo.join(".git").exists());
}

#[tokio::test]
async fn git_annex_subdir() {
    let tmpdir = tempdir().unwrap();
    let tmp_path = tmpdir.path();
    LoggedCommand::new("git", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    LoggedCommand::new("git-annex", ["init"], tmp_path)
        .status()
        .await
        .unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
    let repo = tmp_path.join("foo").join("bar").join("baz");
    ensure_annex_repo(&repo).await.unwrap();
    assert!(tmp_path.join(".git").exists());
    assert!(tmp_path.join(".git").join("annex").exists());
    assert!(!repo.join(".git").exists());
}
