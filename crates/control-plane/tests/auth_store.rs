use olympus_control_plane::auth_store::{AuthStore, Principal};

#[test]
fn bootstraps_admin_and_authenticates_without_storing_plaintext_password() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("auth.sqlite");
    let store = AuthStore::open(&db).unwrap();

    store
        .bootstrap_admin(
            "admin",
            "correct horse battery staple",
            "default",
            "Default",
        )
        .unwrap();

    let principal = store
        .authenticate("admin", "correct horse battery staple")
        .unwrap()
        .expect("valid credentials");
    assert_eq!(principal.username, "admin");
    assert_eq!(principal.kind, Principal::USER_KIND);
    assert!(store.authenticate("admin", "wrong").unwrap().is_none());

    let bytes = std::fs::read(db).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("correct horse battery staple"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("auth.sqlite"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn sessions_are_opaque_expiring_and_revocable() {
    let dir = tempfile::tempdir().unwrap();
    let store = AuthStore::open(&dir.path().join("auth.sqlite")).unwrap();
    store
        .bootstrap_admin("admin", "password-123", "default", "Default")
        .unwrap();
    let user = store
        .authenticate("admin", "password-123")
        .unwrap()
        .unwrap();

    let session = store.create_session(&user.user_id, 1_000, 60).unwrap();
    assert_ne!(session.token, session.token_hash);
    assert_eq!(
        store
            .resolve_session(&session.token, 1_059)
            .unwrap()
            .unwrap()
            .user_id,
        user.user_id
    );
    assert!(store
        .resolve_session(&session.token, 1_061)
        .unwrap()
        .is_none());

    let live = store.create_session(&user.user_id, 2_000, 60).unwrap();
    store.revoke_session(&live.token).unwrap();
    assert!(store.resolve_session(&live.token, 2_001).unwrap().is_none());
}

#[test]
fn organizations_are_filtered_by_membership_and_slugs_are_unique() {
    let dir = tempfile::tempdir().unwrap();
    let store = AuthStore::open(&dir.path().join("auth.sqlite")).unwrap();
    store
        .bootstrap_admin("admin", "password-123", "default", "Default")
        .unwrap();
    let admin = store
        .authenticate("admin", "password-123")
        .unwrap()
        .unwrap();

    let second = store
        .create_organization("acme", "Acme", &admin.user_id)
        .unwrap();
    let organizations = store.organizations_for_user(&admin.user_id).unwrap();
    assert_eq!(organizations.len(), 2);
    assert!(organizations
        .iter()
        .any(|org| org.id == second.id && org.role == "owner"));
    assert!(store
        .user_has_organization(&admin.user_id, &second.id)
        .unwrap());
    assert!(store
        .create_organization("acme", "Duplicate", &admin.user_id)
        .is_err());
}
