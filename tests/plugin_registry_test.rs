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
            requires_env: vec![],
        }],
        hooks: vec![HookCapability {
            directive: "auth".to_string(),
            alias: Some(name.to_string()),
            stage: PluginStage::Prepare,
            cache_scope: PluginCacheScope::None,
            requires_env: vec![],
            parameters_schema: serde_json::json!({"type": "object"}),
        }],
        runtime: None,
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

/// C2（interface-freeze-m1 §1 裁决）：`post_response`/`on_failure`/`harvest` 为 P2
/// 保留字阶段——manifest 中出现即拒绝注册（跳过 + 可见告警），绝不静默注册
/// "永不被调用的死钩子"伪装成可用能力。
#[test]
fn test_manifest_rejects_reserved_stage_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    let raw = serde_json::json!({
        "manifest_version": "1.0",
        "name": "reserved-stage",
        "version": "0.1.0",
        "description": "declares reserved stage",
        "commands": [],
        "hooks": [{
            "directive": "harvest",
            "alias": "reserved-stage",
            "stage": "post_response",
            "cache_scope": "none",
            "requires_env": [],
            "parameters_schema": {}
        }]
    });
    fixture_plugin(&plugin_dir, "reserved-stage", Some(&raw.to_string()));
    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    let reg = PluginRegistry::discover_with_roots(&opts, Some(&[])).unwrap();
    assert!(
        reg.find_hook("reserved-stage").is_none() && reg.find_hook("harvest").is_none(),
        "保留字阶段钩子必须被拒绝注册"
    );
    assert!(
        reg.warnings.iter().any(|w| w.contains("保留字")),
        "拒绝注册必须产生含'保留字'的可见告警: {:?}",
        reg.warnings
    );
}

/// C3（types.rs cache_scope 注释裁决）：manifest 声明 P2 保留值 `ttl` →
/// 不得解析失败，注册成功但 cache_scope 按 none 处理，且必须产生可见告警。
/// 覆盖两种形态：hook 顶层 ttl 字段、cache_scope 内嵌对象（P2 潜在 schema）。
#[test]
fn test_manifest_ttl_reserved_value_warns_and_falls_back_to_none() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    let mk_raw = |name: &str, cache_scope: serde_json::Value, ttl: serde_json::Value| {
        serde_json::json!({
            "manifest_version": "1.0",
            "name": name,
            "version": "0.1.0",
            "description": "declares reserved ttl",
            "commands": [],
            "hooks": [{
                "directive": "auth",
                "alias": name,
                "stage": "prepare",
                "cache_scope": cache_scope,
                "ttl": ttl,
                "requires_env": [],
                "parameters_schema": {}
            }]
        })
    };
    let raw_ttl_hook = mk_raw(
        "ttl-hook",
        serde_json::json!("suite"),
        serde_json::json!(300),
    )
    .to_string();
    fixture_plugin(&plugin_dir, "ttl-hook", Some(raw_ttl_hook.as_str()));
    let raw_ttl_nested = mk_raw(
        "ttl-nested",
        serde_json::json!({"scope": "suite", "ttl": 120}),
        serde_json::json!(null),
    )
    .to_string();
    fixture_plugin(&plugin_dir, "ttl-nested", Some(raw_ttl_nested.as_str()));
    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    let reg = PluginRegistry::discover_with_roots(&opts, Some(&[])).unwrap();

    for name in ["ttl-hook", "ttl-nested"] {
        let (_, hook) = reg
            .find_hook(name)
            .unwrap_or_else(|| panic!("ttl 声明不得导致 {name} 解析失败"));
        assert_eq!(
            hook.cache_scope,
            PluginCacheScope::None,
            "{name} 的 ttl 保留值必须按 cache_scope=none 处理"
        );
    }
    assert_eq!(
        reg.warnings.iter().filter(|w| w.contains("ttl")).count(),
        2,
        "两条 ttl 声明必须各产生一条告警: {:?}",
        reg.warnings
    );
}

#[test]
fn test_manifest_ttl_cache_roundtrip_persists_normalized_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    let raw_ttl = serde_json::json!({
        "manifest_version": "1.0",
        "name": "ttl-cached",
        "version": "0.1.0",
        "description": "ttl normalized cache roundtrip",
        "commands": [],
        "hooks": [{
            "directive": "auth",
            "alias": "ttl-cached",
            "stage": "prepare",
            "cache_scope": {"scope": "suite", "ttl": 120},
            "requires_env": [],
            "parameters_schema": {}
        }]
    })
    .to_string();
    fixture_plugin(&plugin_dir, "ttl-cached", Some(&raw_ttl));

    let cache_path = dir.path().join("rupost_home").join("registry.json");
    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };

    let reg1 = PluginRegistry::load_or_discover_with_roots(&opts, &cache_path, Some(&[])).unwrap();
    assert!(reg1.find_hook("ttl-cached").is_some());

    let cached =
        PluginRegistry::load_cache(&cache_path).expect("normalized ttl plugin must persist");
    assert_eq!(cached.manifests.len(), 1);
    assert_eq!(cached.manifests[0].manifest.name, "ttl-cached");
    assert_eq!(
        cached.manifests[0].manifest.hooks[0].cache_scope,
        PluginCacheScope::None
    );

    let reg2 = PluginRegistry::load_or_discover_with_roots(&opts, &cache_path, Some(&[])).unwrap();
    assert!(
        reg2.warnings.is_empty(),
        "二次加载应直接命中缓存: {:?}",
        reg2.warnings
    );
    assert_eq!(
        reg2.find_hook("ttl-cached").unwrap().1.cache_scope,
        PluginCacheScope::None
    );
}

#[test]
fn test_orphaned_and_invalid_manifests_do_not_cause_cache_thrashing() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join("plugins");
    fixture_plugin(&plugin_dir, "healthy", None);
    // 孤儿 manifest（无同目录二进制）与损坏 JSON
    std::fs::write(
        plugin_dir.join("rupost-plugin-orphan.json"),
        serde_json::to_string(&demo_manifest("orphan")).unwrap(),
    )
    .unwrap();
    std::fs::write(
        plugin_dir.join("rupost-plugin-broken.json"),
        "{not-valid-json",
    )
    .unwrap();

    let cache_path = dir.path().join("rupost_home").join("registry.json");
    let opts = DiscoverOptions::default();

    let reg1 = PluginRegistry::load_or_discover_with_roots(
        &opts,
        &cache_path,
        Some(std::slice::from_ref(&plugin_dir)),
    )
    .unwrap();
    assert!(reg1.find_command("healthy").is_some());
    assert!(reg1.warnings.iter().any(|w| w.contains("orphan")));
    let first_cache_json = std::fs::read_to_string(&cache_path).unwrap();

    // 第二次加载应命中缓存（不重写 cache，也不重复扫描告警）
    let reg2 = PluginRegistry::load_or_discover_with_roots(
        &opts,
        &cache_path,
        Some(std::slice::from_ref(&plugin_dir)),
    )
    .unwrap();
    assert!(reg2.find_command("healthy").is_some());
    assert!(
        reg2.warnings.is_empty(),
        "存在孤儿/损坏 manifest 时不得每次抖动重建缓存: {:?}",
        reg2.warnings
    );
    assert_eq!(
        std::fs::read_to_string(&cache_path).unwrap(),
        first_cache_json
    );

    // 当孤儿 manifest 补齐同目录二进制后，必须立即发现并更新缓存
    let orphan_bin = plugin_dir.join("rupost-plugin-orphan");
    std::fs::write(&orphan_bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&orphan_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let reg3 = PluginRegistry::load_or_discover_with_roots(&opts, &cache_path, Some(&[plugin_dir]))
        .unwrap();
    assert!(
        reg3.find_command("orphan").is_some(),
        "补齐二进制后必须触发缓存刷新并发现 orphan"
    );
}

#[test]
fn test_fingerprint_detects_changes_beyond_4kb_and_subsecond_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("plugins");
    let mut big_manifest = demo_manifest("big");
    big_manifest.description = "A".repeat(8192);
    let manifest_path = fixture_plugin(
        &root,
        "big",
        Some(&serde_json::to_string(&big_manifest).unwrap()),
    );

    let mut reg = PluginRegistry::new();
    reg.register_manifest_with_source(
        root.join("rupost-plugin-big"),
        manifest_path.clone(),
        big_manifest.clone(),
    );
    let cache_path = dir.path().join("rupost_home").join("registry.json");
    reg.persist_cache(&cache_path);
    assert!(PluginRegistry::load_cache(&cache_path).is_some());

    // 1. 保持精确相同的 mtime（秒与纳秒均不变），仅篡改 4KB 之后的 1 个字节 -> 必须失效
    let orig_meta = std::fs::metadata(&manifest_path).unwrap();
    let orig_mtime = filetime::FileTime::from_last_modification_time(&orig_meta);
    let mut modified_manifest = big_manifest.clone();
    modified_manifest.description.replace_range(7000..7001, "B");
    let modified_json = serde_json::to_string(&modified_manifest).unwrap();
    std::fs::write(&manifest_path, &modified_json).unwrap();
    filetime::set_file_mtime(&manifest_path, orig_mtime).unwrap();
    assert!(
        PluginRegistry::load_cache(&cache_path).is_none(),
        "超过 4KB 处的单字节修改即使 mtime 相同也必须失效"
    );

    // 2. 恢复原内容并保持 mtime 秒数不变、仅改变亚秒纳秒 -> 也必须失效
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&big_manifest).unwrap(),
    )
    .unwrap();
    let shifted_nanos = if orig_mtime.nanoseconds() >= 500_000_000 {
        orig_mtime.nanoseconds() - 100_000_000
    } else {
        orig_mtime.nanoseconds() + 100_000_000
    };
    let subsec_mtime = filetime::FileTime::from_unix_time(orig_mtime.unix_seconds(), shifted_nanos);
    filetime::set_file_mtime(&manifest_path, subsec_mtime).unwrap();
    assert!(
        PluginRegistry::load_cache(&cache_path).is_none(),
        "亚秒级 mtime 变化必须导致缓存失效"
    );
}

#[test]
fn test_cache_hit_revalidates_executable_confinement_and_reserved_stages() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("plugins");
    let manifest_path = fixture_plugin(&root, "victim", None);
    let manifest_canon = manifest_path.canonicalize().unwrap();
    let (mtime_secs, fingerprint_hash) =
        rupost::plugin::registry::manifest_fingerprint(&manifest_canon).unwrap();

    // 构造外部逃逸可执行路径的投毒缓存
    let outside_exe = dir.path().join("evil-outside");
    std::fs::write(&outside_exe, "#!/bin/sh\nexit 0\n").unwrap();
    let cache_path = dir.path().join("rupost_home").join("poison.json");
    let poisoned_escape = RegistryCacheFile {
        version: "1.0".to_string(),
        updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
        manifests: vec![CachedPluginEntry {
            executable_path: outside_exe.to_string_lossy().into_owned(),
            manifest_path: manifest_canon.to_string_lossy().into_owned(),
            mtime_secs,
            fingerprint_hash: fingerprint_hash.clone(),
            manifest: demo_manifest("victim"),
        }],
    };
    PluginRegistry::save_cache_atomic(&poisoned_escape, &cache_path).unwrap();

    let reg = PluginRegistry::load_or_discover_with_roots(
        &DiscoverOptions::default(),
        &cache_path,
        Some(std::slice::from_ref(&root)),
    )
    .unwrap();
    let (resolved_exe, _) = reg.find_command("victim").unwrap();
    assert_ne!(
        resolved_exe, &outside_exe,
        "缓存命中复核必须拒绝逃逸插件根的可执行路径"
    );

    // 构造包含 P2 保留阶段钩子的投毒缓存
    let mut bad_manifest = demo_manifest("victim");
    bad_manifest.hooks[0].stage = PluginStage::PostResponse;
    let poisoned_stage = RegistryCacheFile {
        version: "1.0".to_string(),
        updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
        manifests: vec![CachedPluginEntry {
            executable_path: root
                .join("rupost-plugin-victim")
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            manifest_path: manifest_canon.to_string_lossy().into_owned(),
            mtime_secs,
            fingerprint_hash,
            manifest: bad_manifest,
        }],
    };
    PluginRegistry::save_cache_atomic(&poisoned_stage, &cache_path).unwrap();

    let reg2 = PluginRegistry::load_or_discover_with_roots(
        &DiscoverOptions::default(),
        &cache_path,
        Some(&[root]),
    )
    .unwrap();
    assert_eq!(
        reg2.find_hook("victim").unwrap().1.stage,
        PluginStage::Prepare,
        "缓存命中复核必须拒绝保留字阶段并回退重新发现"
    );
}

#[cfg(unix)]
#[test]
fn test_untrusted_world_writable_plugin_dir_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let untrusted_root = dir.path().join("world_writable");
    fixture_plugin(&untrusted_root, "sneaky", None);
    std::fs::set_permissions(&untrusted_root, std::fs::Permissions::from_mode(0o777)).unwrap();

    let reg = PluginRegistry::discover_with_roots(
        &DiscoverOptions::default(),
        Some(std::slice::from_ref(&untrusted_root)),
    )
    .unwrap();
    assert!(
        reg.find_command("sneaky").is_none(),
        "全局可写插件目录必须被拒绝加载"
    );
    assert!(
        reg.warnings.iter().any(|w| w.contains("不可信")),
        "拒绝不可信目录必须产生告警: {:?}",
        reg.warnings
    );

    // 还原权限以免 tempdir 清理或后续检查受影响
    let _ = std::fs::set_permissions(&untrusted_root, std::fs::Permissions::from_mode(0o755));
}

#[test]
fn test_multistage_hooks_same_plugin_do_not_collide() {
    let mut reg = PluginRegistry::new();
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: "dual-stage".to_string(),
        version: "0.1.0".to_string(),
        description: "Multi-stage hooks on one plugin".to_string(),
        commands: vec![],
        hooks: vec![
            HookCapability {
                directive: "auth".to_string(),
                alias: Some("dual-stage".to_string()),
                stage: PluginStage::Prepare,
                cache_scope: PluginCacheScope::Suite,
                requires_env: vec![],
                parameters_schema: serde_json::json!({"phase": "prepare"}),
            },
            HookCapability {
                directive: "sign".to_string(),
                alias: Some("dual-stage".to_string()),
                stage: PluginStage::Sign,
                cache_scope: PluginCacheScope::None,
                requires_env: vec![],
                parameters_schema: serde_json::json!({"phase": "sign"}),
            },
        ],
        runtime: None,
    };
    reg.register_manifest(PathBuf::from("/usr/local/bin/rupost-plugin-dual"), manifest);

    assert!(
        reg.warnings.is_empty(),
        "同插件不同阶段钩子不得触发冲突告警: {:?}",
        reg.warnings
    );
    let (_, prep) = reg
        .find_hook_for_stage("dual-stage", PluginStage::Prepare)
        .expect("prepare hook");
    assert_eq!(prep.cache_scope, PluginCacheScope::Suite);
    assert_eq!(prep.directive, "auth");

    let (_, sign) = reg
        .find_hook_for_stage("dual-stage", PluginStage::Sign)
        .expect("sign hook");
    assert_eq!(sign.cache_scope, PluginCacheScope::None);
    assert_eq!(sign.directive, "sign");
}

#[test]
fn test_preflight_runtime_zero_subprocess_and_unix_exec_bit() {
    use rupost::plugin::registry::preflight_runtime;
    use rupost::plugin::types::PluginRuntime;
    use std::ffi::OsStr;

    // 1. None 或无 interpreter 声明视为原生二进制，直接 Ready
    assert!(preflight_runtime(None, None).is_ok());
    let native_rt = PluginRuntime::default();
    assert!(preflight_runtime(Some(&native_rt), None).is_ok());

    // 2. 空/缺失 PATH
    let py_rt = PluginRuntime {
        interpreter: Some("fake-py3".to_string()),
        min_version: Some("3.9".to_string()),
        modules: vec!["boto3".to_string()],
    };
    assert_eq!(
        preflight_runtime(Some(&py_rt), None),
        Err(("fake-py3".to_string(), 0))
    );
    assert_eq!(
        preflight_runtime(Some(&py_rt), Some(OsStr::new(""))),
        Err(("fake-py3".to_string(), 0))
    );

    // 3. PATH 目录中存在同名普通文件但无执行权限（Unix mode & 0o111 == 0）必须判 Pending
    let dir = tempfile::tempdir().unwrap();
    let bin1 = dir.path().join("bin1");
    let bin2 = dir.path().join("bin2");
    std::fs::create_dir_all(&bin1).unwrap();
    std::fs::create_dir_all(&bin2).unwrap();
    let fake_bin = bin1.join("fake-py3");
    // 写入若被执行就会创建标记文件或退出 99 的脚本，校验零子进程预检绝不执行它
    let probe_marker = dir.path().join("spawned.marker");
    std::fs::write(
        &fake_bin,
        format!("#!/bin/sh\ntouch \"{}\"\nexit 99\n", probe_marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    let joined_path = std::env::join_paths([&bin1, &bin2]).unwrap();
    #[cfg(unix)]
    assert_eq!(
        preflight_runtime(Some(&py_rt), Some(joined_path.as_os_str())),
        Err(("fake-py3".to_string(), 2))
    );

    // 4. 加上 +x 后立即可用，且绝不 spawn --version
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(preflight_runtime(Some(&py_rt), Some(joined_path.as_os_str())).is_ok());
    assert!(
        !probe_marker.exists(),
        "preflight_runtime 绝不能 spawn 解释器子进程"
    );
}

#[test]
fn test_evaluate_statuses_three_states_reactive_and_no_cache_status_leak() {
    use rupost::plugin::types::{PluginHealthStatus, PluginRuntime};

    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join(".rupost").join("plugins");
    let fake_bin_dir = dir.path().join("fake_path");
    std::fs::create_dir_all(&fake_bin_dir).unwrap();

    // 1. Ready 原生插件
    fixture_plugin(&plugin_dir, "native-ok", None);

    // 2. Pending 解释器插件
    let mut py_manifest = demo_manifest("py-pending");
    py_manifest.runtime = Some(PluginRuntime {
        interpreter: Some("missing-interp-m6".to_string()),
        min_version: Some("3.11".to_string()),
        modules: vec!["requests".to_string(), "boto3".to_string()],
    });
    fixture_plugin(
        &plugin_dir,
        "py-pending",
        Some(&serde_json::to_string(&py_manifest).unwrap()),
    );

    // 3. Failed 损坏 manifest
    std::fs::write(plugin_dir.join("rupost-plugin-broken-m6.json"), "{bad json").unwrap();

    let opts = DiscoverOptions {
        allow_project_plugins: true,
        project_root: Some(dir.path().to_path_buf()),
    };
    let cache_path = dir.path().join("rupost_home").join("registry.json");

    let mut reg = PluginRegistry::discover_with_roots(&opts, Some(&[])).unwrap();
    assert_eq!(reg.failed_plugins().len(), 1);
    reg.persist_cache(&cache_path);

    let cache_raw = std::fs::read_to_string(&cache_path).unwrap();
    assert!(
        !cache_raw.contains("\"ready\"")
            && !cache_raw.contains("\"pending\"")
            && !cache_raw.contains("\"status\""),
        "registry.json 绝不能持久化 ready/pending 状态字段: {cache_raw}"
    );

    let statuses_before = reg.evaluate_statuses(Some(fake_bin_dir.as_os_str()));
    assert_eq!(statuses_before.len(), 3);
    assert!(
        statuses_before
            .iter()
            .any(|e| e.name == "native-ok" && e.status == PluginHealthStatus::Ready)
    );
    assert!(statuses_before.iter().any(|e| {
        e.name == "py-pending"
            && matches!(
                &e.status,
                PluginHealthStatus::Pending {
                    interpreter,
                    min_version,
                    modules,
                    searched_dirs: 1,
                } if interpreter == "missing-interp-m6"
                    && min_version.as_deref() == Some("3.11")
                    && modules == &vec!["requests".to_string(), "boto3".to_string()]
            )
    }));
    assert!(
        statuses_before
            .iter()
            .any(|e| matches!(e.status, PluginHealthStatus::Failed { .. }))
    );

    // 响应式重算：安装解释器后，同一注册表（或缓存加载后的注册表）无需重建即自动转 Ready
    let interp_path = fake_bin_dir.join("missing-interp-m6");
    std::fs::write(&interp_path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&interp_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let statuses_after = reg.evaluate_statuses(Some(fake_bin_dir.as_os_str()));
    assert!(
        statuses_after
            .iter()
            .any(|e| e.name == "py-pending" && e.status == PluginHealthStatus::Ready),
        "解释器安装后 evaluate_statuses 必须响应式转为 Ready: {statuses_after:?}"
    );
}

#[test]
fn test_collect_missing_env_warnings_deduplicated_and_respects_allowlists() {
    use std::collections::HashSet;

    let mut reg = PluginRegistry::new();
    let manifest = PluginManifest {
        manifest_version: "1.0".to_string(),
        name: "aws-sigv4".to_string(),
        version: "0.1.0".to_string(),
        description: "signer".to_string(),
        commands: vec![CommandCapability {
            name: "aws-cmd".to_string(),
            usage: "rupost aws-cmd".to_string(),
            description: "cmd".to_string(),
            requires_tty: false,
            requires_env: vec!["CMD_SECRET".to_string(), "PATH".to_string()],
        }],
        hooks: vec![
            HookCapability {
                directive: "sign".to_string(),
                alias: Some("aws-sigv4".to_string()),
                stage: PluginStage::Sign,
                cache_scope: PluginCacheScope::None,
                requires_env: vec![
                    "AWS_PROFILE".to_string(),
                    "AWS_REGION".to_string(),
                    "RUPOST_ENV".to_string(),
                    "HOME".to_string(),
                ],
                parameters_schema: serde_json::json!({}),
            },
            HookCapability {
                directive: "auth".to_string(),
                alias: Some("unused-hook".to_string()),
                stage: PluginStage::Prepare,
                cache_scope: PluginCacheScope::None,
                requires_env: vec!["UNUSED_SECRET".to_string()],
                parameters_schema: serde_json::json!({}),
            },
        ],
        runtime: None,
    };
    reg.register_manifest(PathBuf::from("/bin/rupost-plugin-aws-sigv4"), manifest);

    let used = vec![
        "aws-sigv4".to_string(),
        "aws-sigv4".to_string(),
        "aws-cmd".to_string(),
    ];
    let mut auth = HashSet::from(["AWS_REGION".to_string()]);
    let warnings = reg.collect_missing_env_warnings(&used, &auth);
    assert_eq!(warnings.len(), 2, "应去重且仅告警未放行键: {warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("AWS_PROFILE")));
    assert!(warnings.iter().any(|w| w.contains("CMD_SECRET")));
    assert!(!warnings.iter().any(|w| w.contains("AWS_REGION")));
    assert!(!warnings.iter().any(|w| w.contains("RUPOST_ENV")));
    assert!(!warnings.iter().any(|w| w.contains("HOME")));
    assert!(!warnings.iter().any(|w| w.contains("PATH")));
    assert!(!warnings.iter().any(|w| w.contains("UNUSED_SECRET")));

    auth.insert("AWS_PROFILE".to_string());
    auth.insert("CMD_SECRET".to_string());
    assert!(reg.collect_missing_env_warnings(&used, &auth).is_empty());
}
