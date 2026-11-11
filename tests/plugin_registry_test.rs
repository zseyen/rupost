//! Step 2 守护与迁移测试：PluginRegistry 静态发现、路径穿越拒绝、缓存指纹/权限/symlink 防线（零全局 PATH 修改）。

use rupost::plugin::registry::{CachedPluginEntry, RegistryCacheFile};
use rupost::plugin::types::{
    CommandCapability, HookCapability, PluginCacheScope, PluginManifest, PluginStage,
};
use rupost::plugin::{DiscoverOptions, PluginRegistry};
use std::path::{Path, PathBuf};

fn demo_manifest(name: &str) -> PluginManifest {
    PluginManifest {
        manifest_version: "1.0".to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: "Demo plugin".to_string(),
        commands: vec![CommandCapability {
            name: name.to_string(),
            usage: format!("rupost {name}"),
            description: "Demo command".to_string(),
            requires_tty: false,
        }],
        hooks: vec![HookCapability {
            directive: "auth".to_string(),
            alias: Some(name.to_string()),
            stage: PluginStage::Prepare,
            cache_scope: PluginCacheScope::None,
            requires_env: vec![],
            parameters_schema: serde_json::json!({"type": "object"}),
        }],
    }
}

fn fixture_plugin(root: &Path, plugin_name: &str, body: Option<&str>) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    let bin = root.join(format!("rupost-plugin-{plugin_name}"));
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest_path = root.join(format!("rupost-plugin-{plugin_name}.json"));
    let manifest = body
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string(&demo_manifest(plugin_name)).unwrap());
    std::fs::write(&manifest_path, manifest).unwrap();
    manifest_path
}

#[test]
fn test_persist_cache_tracks_real_manifest_for_fallback_executable_name() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let bin = plugin_dir.join("rupost-step2fallback");
    std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let manifest_path = plugin_dir.join("rupost-plugin-step2fallback.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&demo_manifest("step2fallback")).unwrap(),
    )
    .unwrap();

    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    let cache_path = dir.path().join("registry.json");
    let mut reg = PluginRegistry::discover_with_roots(&opts, Some(&[])).unwrap();
    assert!(reg.find_hook("step2fallback").is_some());

    reg.persist_cache(&cache_path);
    let cache = PluginRegistry::load_cache(&cache_path).expect("cache saved");
    assert!(
        cache.manifests.iter().any(|e| {
            e.manifest.name == "step2fallback"
                && e.manifest_path
                    .ends_with("rupost-plugin-step2fallback.json")
        }),
        "persist_cache 必须追踪真实 manifest 路径，而非 exe.with_extension(json): {:?}",
        cache.manifests
    );
}

#[cfg(unix)]
#[test]
fn test_cache_rejects_symlink_target() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real_file = dir.path().join("real.json");
    std::fs::write(
        &real_file,
        r#"{"version":"1.0","updated_at_utc":"2026-01-01T00:00:00Z","manifests":[]}"#,
    )
    .unwrap();
    std::fs::set_permissions(&real_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let symlink_cache = dir.path().join("registry.json");
    std::os::unix::fs::symlink(&real_file, &symlink_cache).unwrap();

    assert!(
        PluginRegistry::load_cache(&symlink_cache).is_none(),
        "symlink 缓存路径必须被 symlink_metadata 拒绝"
    );
}

#[test]
fn test_discovery_static_manifest_and_path_traversal_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("plugins");
    let _good = fixture_plugin(&root, "good", None);
    fixture_plugin(
        &root,
        "evil-dotdot",
        Some(
            r#"{"manifest_version":"1.0","name":"evil-dotdot","version":"1","description":"x","commands":[],"hooks":[]}"#,
        ),
    );

    let outside = dir.path().join("outside.sh");
    std::fs::write(&outside, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("rupost-plugin-evil-symlink")).unwrap();
    std::fs::write(
        root.join("rupost-plugin-evil-symlink.json"),
        r#"{"manifest_version":"1.0","name":"evil-symlink","version":"1","description":"x","commands":[{"name":"boom","usage":"u","description":"d"}],"hooks":[]}"#,
    )
    .unwrap();

    let options = DiscoverOptions {
        project_root: Some(dir.path().to_path_buf()),
        allow_project_plugins: true,
    };
    let registry = PluginRegistry::discover_with_roots(&options, Some(&[root])).unwrap();
    assert!(
        registry.find_command("good").is_some(),
        "{:?}",
        registry.warnings
    );
    assert!(registry.find_command("boom").is_none());
    assert!(registry.warnings.iter().any(|w| w.contains("evil-symlink")));
}

#[cfg(unix)]
#[test]
fn test_cache_rejects_foreign_owner_and_loose_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("registry.json");
    let cache = RegistryCacheFile {
        version: "1.0".to_string(),
        updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
        manifests: vec![],
    };
    PluginRegistry::save_cache_atomic(&cache, &cache_path).unwrap();

    std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(PluginRegistry::load_cache(&cache_path).is_none());

    std::fs::set_permissions(&cache_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(PluginRegistry::load_cache(&cache_path).is_some());
}

#[test]
fn test_cache_staleness_and_empty_cache_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("plugins");
    let manifest_path = fixture_plugin(&root, "fresh", None);

    let mut registry = PluginRegistry::new();
    registry.register_manifest_with_source(
        root.join("rupost-plugin-fresh"),
        manifest_path.clone(),
        demo_manifest("fresh"),
    );
    let cache_path = dir.path().join("registry.json");
    registry.persist_cache(&cache_path);
    assert!(PluginRegistry::load_cache(&cache_path).is_some());

    let mut updated = demo_manifest("fresh");
    updated.hooks[0].cache_scope = PluginCacheScope::Suite;
    std::fs::write(&manifest_path, serde_json::to_string(&updated).unwrap()).unwrap();
    assert!(PluginRegistry::load_cache(&cache_path).is_none());

    // 空缓存 + 磁盘新增插件必须触发重建发现
    let cache_path2 = dir.path().join("empty_registry.json");
    PluginRegistry::save_cache_atomic(&RegistryCacheFile::default(), &cache_path2).unwrap();
    let pdir = dir.path().join(".rupost").join("plugins");
    fixture_plugin(&pdir, "newcomer", None);
    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    let rebuilt =
        PluginRegistry::load_or_discover_with_roots(&opts, &cache_path2, Some(&[])).unwrap();
    assert!(rebuilt.find_command("newcomer").is_some());
}

#[test]
fn test_cache_not_authority_and_project_gate_and_alias_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("poison.json");
    let poisoned = RegistryCacheFile {
        version: "1.0".to_string(),
        updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
        manifests: vec![CachedPluginEntry {
            executable_path: "/tmp/evil".to_string(),
            manifest_path: "/tmp/evil.json".to_string(),
            mtime_secs: 0,
            fingerprint_hash: "deadbeef".to_string(),
            manifest: demo_manifest("evil"),
        }],
    };
    PluginRegistry::save_cache_atomic(&poisoned, &cache_path).unwrap();
    assert!(PluginRegistry::load_cache(&cache_path).is_none());

    let pdir = dir.path().join(".rupost").join("plugins");
    fixture_plugin(&pdir, "local", None);
    let closed = DiscoverOptions {
        allow_project_plugins: false,
        project_root: Some(dir.path().to_path_buf()),
    };
    assert!(
        PluginRegistry::discover_with_roots(&closed, Some(&[]))
            .unwrap()
            .commands
            .is_empty()
    );
    let opened = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    assert!(
        PluginRegistry::discover_with_roots(&opened, Some(&[]))
            .unwrap()
            .find_command("local")
            .is_some()
    );

    let mut dup_reg = PluginRegistry::new();
    dup_reg.register_manifest(PathBuf::from("/a/rupost-plugin-dup"), demo_manifest("dup"));
    dup_reg.register_manifest(PathBuf::from("/b/rupost-plugin-dup"), demo_manifest("dup"));
    assert_eq!(
        dup_reg.find_hook("dup").unwrap().0,
        PathBuf::from("/b/rupost-plugin-dup")
    );
    assert!(dup_reg.warnings.iter().any(|w| w.contains("冲突")));
}

#[test]
fn test_registry_registration_and_lookup() {
    let mut registry = PluginRegistry::new();
    registry.register_manifest(
        PathBuf::from("/usr/local/bin/rupost-plugin-aws-sigv4"),
        demo_manifest("aws-sigv4"),
    );
    assert!(registry.find_command("aws-sigv4").is_some());
    assert_eq!(
        registry.find_hook("aws-sigv4").unwrap().1.stage,
        PluginStage::Prepare
    );
}
