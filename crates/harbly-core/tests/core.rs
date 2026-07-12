use harbly_core::{Library, SortKey};
use std::fs;

fn html(title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><title>{title}</title></head><body><h2>{body}</h2><script>var x=1;</script></body></html>")
}

/// Build a Markdown document, optionally with a YAML front-matter title.
fn md(front_title: Option<&str>, body: &str) -> String {
    match front_title {
        Some(t) => format!("---\ntitle: {t}\n---\n\n{body}"),
        None => body.to_string(),
    }
}

fn setup() -> (tempfile::TempDir, Library) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Harbly");
    let lib = Library::open_or_create(&root).unwrap();
    (tmp, lib)
}

#[test]
fn scan_index_search_chinese() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("客户项目/星链SaaS")).unwrap();
    fs::write(
        root.join("客户项目/星链SaaS/季度营收仪表盘.html"),
        html("季度营收仪表盘", "Q2 营收总览，定价策略与净收入留存"),
    )
    .unwrap();
    fs::write(
        root.join("dashboard.html"),
        html("Revenue Dashboard", "quarterly pricing overview"),
    )
    .unwrap();

    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.added, 2);
    assert_eq!(sum.total, 2);

    // Two-character Chinese word search (FTS5 can't do this natively; verifies the jieba path)
    let hits = lib.search("定价").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset.title, "季度营收仪表盘");

    // English prefix search
    let hits = lib.search("pric").unwrap();
    assert_eq!(hits.len(), 1);

    // Directory tree and counts
    let tree = lib.dir_tree().unwrap();
    assert_eq!(tree.count, 2);
    let proj = tree.children.iter().find(|c| c.name == "客户项目").unwrap();
    assert_eq!(proj.count, 1);
    assert_eq!(proj.children[0].name, "星链SaaS");

    // Listing: empty string = "All Assets", the whole library recursively; concrete folder = direct files only
    let all = lib.list_assets("", SortKey::Recent).unwrap();
    assert_eq!(all.len(), 2);
    let in_folder = lib
        .list_assets("客户项目/星链SaaS", SortKey::Recent)
        .unwrap();
    assert_eq!(in_folder.len(), 1);
    assert_eq!(in_folder[0].file_name, "季度营收仪表盘.html");
}

#[test]
fn import_dedup_and_rename() {
    let (tmp, lib) = setup();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.html"), html("A", "内容甲")).unwrap();
    fs::write(outside.join("b.html"), html("B", "内容乙")).unwrap();
    fs::write(outside.join("readme.txt"), "not html").unwrap();

    let r = lib
        .import_files(
            &[
                outside.join("a.html"),
                outside.join("b.html"),
                outside.join("readme.txt"),
            ],
            "",
        )
        .unwrap();
    assert_eq!(r.added, 2);
    assert_eq!(r.skipped, 1);

    // Re-importing identical content → deduplicated
    let r2 = lib.import_files(&[outside.join("a.html")], "").unwrap();
    assert_eq!(r2.added, 0);
    assert_eq!(r2.duplicates, 1);

    // Same name, different content → auto-suffixed
    fs::write(outside.join("a.html"), html("A2", "内容甲改")).unwrap();
    let r3 = lib.import_files(&[outside.join("a.html")], "").unwrap();
    assert_eq!(r3.added, 1);
    assert_eq!(r3.renamed, 1);
    assert!(lib.root().join("a-2.html").exists());
}

#[test]
fn version_chain_external_edit_and_restore() {
    let (_tmp, lib) = setup();
    let f = lib.root().join("page.html");
    fs::write(&f, html("Page", "第一版内容")).unwrap();
    lib.scan(|_| {}).unwrap();

    let assets = lib.list_assets("", SortKey::Recent).unwrap();
    let id = assets[0].id.clone();
    assert_eq!(assets[0].ver_count, 1);

    // External edit (different length → size change triggers re-hashing)
    fs::write(&f, html("Page", "第二版内容，加长一些以确保 size 变化")).unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.updated, 1);
    let vs = lib.list_versions(&id).unwrap();
    assert_eq!(vs.len(), 2);
    assert_eq!(vs[0].label, "外部修改");

    // Roll back to v1: content restored, and history gains an appended v3
    lib.restore_version(&id, 1).unwrap();
    let vs = lib.list_versions(&id).unwrap();
    assert_eq!(vs.len(), 3);
    let now = fs::read_to_string(&f).unwrap();
    assert!(now.contains("第一版内容"));

    // Version files are full snapshots
    let v2 = fs::read_to_string(lib.version_file_path(&id, 2)).unwrap();
    assert!(v2.contains("第二版内容"));
}

#[test]
fn folder_rename_and_duplicate() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("项目/子目录")).unwrap();
    fs::write(root.join("项目/首页.html"), html("首页", "甲")).unwrap();
    fs::write(root.join("项目/子目录/内页.html"), html("内页", "乙")).unwrap();
    lib.scan(|_| {}).unwrap();

    let before = lib.list_assets("项目", SortKey::Name).unwrap();
    let id = before[0].id.clone();

    // Rename a folder: paths rebound, asset ids unchanged, nothing deleted
    lib.rename_folder("项目", "项目X").unwrap();
    assert!(root.join("项目X/首页.html").exists());
    let after = lib.asset(&id).unwrap();
    assert_eq!(after.folder, "项目X");
    let tree = lib.dir_tree().unwrap();
    assert!(tree.children.iter().any(|c| c.name == "项目X"));

    // Duplicate a folder: recursive copy + new assets registered
    let dup = lib.duplicate_folder("项目X").unwrap();
    assert_eq!(dup, "项目X 副本");
    assert!(root.join("项目X 副本/子目录/内页.html").exists());
    assert_eq!(lib.total_count().unwrap(), 4);

    // Duplicate a file: same-directory copy with the "副本" suffix
    let copy = lib.duplicate_asset(&id).unwrap();
    assert!(copy.file_name.contains("副本"));
    assert_eq!(copy.folder, "项目X");
}

#[test]
fn tags_and_export() {
    let (tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::create_dir_all(root.join("目录")).unwrap();
    fs::write(root.join("目录/甲.html"), html("甲", "内容甲")).unwrap();
    fs::write(root.join("目录/乙.html"), html("乙", "内容乙")).unwrap();
    lib.scan(|_| {}).unwrap();

    let assets = lib.list_assets("目录", SortKey::Name).unwrap();
    assert!(assets[0].tags.is_empty());

    // Tag → tags come back in metadata → tag list → fetch by tag
    lib.set_tags(&assets[0].id, &["仪表盘".into(), "重要".into()])
        .unwrap();
    lib.set_tags(&assets[1].id, &["重要".into()]).unwrap();
    let a = lib.asset(&assets[0].id).unwrap();
    assert_eq!(a.tags.len(), 2);
    let tags = lib.all_tags().unwrap();
    assert_eq!(tags[0].name, "重要");
    assert_eq!(tags[0].count, 2);
    assert_eq!(lib.assets_by_tag("仪表盘").unwrap().len(), 1);

    // Search hits on a tag word
    let hits = lib.search("仪表盘").unwrap();
    assert!(hits.iter().any(|h| h.asset.id == assets[0].id));

    // Export zip
    let dest = tmp.path().join("out.zip");
    let n = lib.export_folder_zip("目录", &dest).unwrap();
    assert_eq!(n, 2);
    assert!(dest.metadata().unwrap().len() > 0);
}

#[test]
fn favorites_star_persist_and_rescan() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(root.join("a.html"), html("A", "内容甲")).unwrap();
    fs::write(root.join("b.html"), html("B", "内容乙")).unwrap();
    lib.scan(|_| {}).unwrap();
    let assets = lib.list_assets("", harbly_core::SortKey::Name).unwrap();
    let (a, b) = (&assets[0], &assets[1]);
    assert!(!a.favorite);

    // Star → metadata, favorites view, count, and the on-file xattr all agree
    lib.set_favorite(&a.id, true).unwrap();
    assert!(lib.asset(&a.id).unwrap().favorite);
    let favs = lib.favorite_assets().unwrap();
    assert_eq!(favs.len(), 1);
    assert_eq!(favs[0].id, a.id);
    assert_eq!(lib.favorite_count().unwrap(), 1);
    assert!(harbly_core::read_file_favorite(&root.join(&a.rel_path)));

    // Disk is the source of truth: star b externally (another machine / a
    // restored backup), rescan adopts it
    harbly_core::write_file_favorite(&root.join(&b.rel_path), true).unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert!(sum.tags_synced >= 1);
    assert!(lib.asset(&b.id).unwrap().favorite);
    assert_eq!(lib.favorite_count().unwrap(), 2);

    // The duplicate inherits the star (xattrs travel with copies)
    let copy = lib.duplicate_asset(&a.id).unwrap();
    assert!(lib.asset(&copy.id).unwrap().favorite);

    // Unstar removes the xattr, not just the cache
    lib.set_favorite(&a.id, false).unwrap();
    assert!(!lib.asset(&a.id).unwrap().favorite);
    assert!(!harbly_core::read_file_favorite(&root.join(&a.rel_path)));
}

#[test]
fn move_rename_inbox() {
    let (_tmp, lib) = setup();
    fs::write(
        lib.root().join("_inbox").join("捕获.html"),
        html("新捕获", "插件收藏内容"),
    )
    .unwrap();
    fs::write(lib.root().join("x.html"), html("X", "正文")).unwrap();
    lib.scan(|_| {}).unwrap();
    assert_eq!(lib.inbox_count().unwrap(), 1);

    // Archiving = moving out of the inbox
    let inbox = lib.list_assets("_inbox", SortKey::Recent).unwrap();
    lib.create_folder("", "项目A").unwrap();
    let moved = lib.move_asset(&inbox[0].id, "项目A").unwrap();
    assert_eq!(moved.folder, "项目A");
    assert!(lib.root().join("项目A/捕获.html").exists());
    assert_eq!(lib.inbox_count().unwrap(), 0);

    // Rename keeps the extension
    let all = lib.list_assets("", SortKey::Recent).unwrap();
    let x = all.iter().find(|a| a.file_name == "x.html").unwrap();
    let renamed = lib.rename_asset(&x.id, "改名").unwrap();
    assert_eq!(renamed.file_name, "改名.html");
    assert!(lib.root().join("改名.html").exists());

    // Finder move (simulated): raw rename then rescan; rebound by hash without losing versions
    fs::rename(
        lib.root().join("改名.html"),
        lib.root().join("项目A/改名.html"),
    )
    .unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.moved, 1);
    assert_eq!(sum.removed, 0);
    let a = lib.list_assets("项目A", SortKey::Name).unwrap();
    assert_eq!(a.len(), 2);
}

#[test]
fn md_scan_title_precedence_and_chinese_fts() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    // Front-matter title wins over the H1
    fs::write(
        root.join("frontmatter.md"),
        md(Some("我的笔记"), "# 别的标题\n\n正文包含 定价 策略与净收入"),
    )
    .unwrap();
    // No front matter → first H1 is the title
    fs::write(
        root.join("heading.md"),
        md(None, "# 季度报告\n\n一些中文内容"),
    )
    .unwrap();
    // Neither → falls back to the file stem
    fs::write(
        root.join("plain.md"),
        md(None, "just plain text, no heading"),
    )
    .unwrap();

    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.added, 3);

    let all = lib.list_assets("", SortKey::Name).unwrap();
    let title_of = |name: &str| {
        all.iter()
            .find(|a| a.file_name == name)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(title_of("frontmatter.md"), "我的笔记");
    assert_eq!(title_of("heading.md"), "季度报告");
    assert_eq!(title_of("plain.md"), "plain"); // file stem

    // Two-character Chinese word search hits the Markdown body (jieba path)
    let hits = lib.search("定价").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset.file_name, "frontmatter.md");
}

#[test]
fn md_rename_preserves_extension() {
    let (_tmp, lib) = setup();
    fs::write(lib.root().join("note.md"), md(None, "# 甲\n\n内容")).unwrap();
    fs::write(lib.root().join("page.html"), html("Page", "content")).unwrap();
    lib.scan(|_| {}).unwrap();

    let all = lib.list_assets("", SortKey::Name).unwrap();
    let note = all.iter().find(|a| a.file_name == "note.md").unwrap();
    let page = all.iter().find(|a| a.file_name == "page.html").unwrap();

    // Markdown keeps .md even without a typed extension
    let r = lib.rename_asset(&note.id, "改名").unwrap();
    assert_eq!(r.file_name, "改名.md");
    // Typing the wrong managed extension does not convert the type
    let r = lib.rename_asset(&note.id, "again.html").unwrap();
    assert_eq!(r.file_name, "again.md");
    // HTML still behaves as before
    let r = lib.rename_asset(&page.id, "renamed").unwrap();
    assert_eq!(r.file_name, "renamed.html");

    // An extension-only rename is rejected (would create a hidden ".md" file)
    assert!(lib.rename_asset(&note.id, ".md").is_err());
    assert!(lib.rename_asset(&note.id, ".html").is_err());
    assert!(lib.root().join("again.md").exists()); // unchanged from the earlier rename
}

#[test]
fn md_version_chain_uses_md_extension() {
    let (_tmp, lib) = setup();
    let f = lib.root().join("doc.md");
    fs::write(&f, md(None, "# 标题\n\n第一版")).unwrap();
    lib.scan(|_| {}).unwrap();
    let id = lib.list_assets("", SortKey::Recent).unwrap()[0].id.clone();

    fs::write(
        &f,
        md(None, "# 标题\n\n第二版内容，加长一些以确保 size 变化"),
    )
    .unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.updated, 1);

    // Version snapshots carry the asset's own extension
    assert!(lib.version_file_path(&id, 1).exists());
    assert!(lib.version_file_path(&id, 2).ends_with("v2.md"));
    let v2 = fs::read_to_string(lib.version_file_path(&id, 2)).unwrap();
    assert!(v2.contains("第二版内容"));

    // Rollback restores content and appends a new version
    lib.restore_version(&id, 1).unwrap();
    assert!(fs::read_to_string(&f).unwrap().contains("第一版"));
    assert_eq!(lib.list_versions(&id).unwrap().len(), 3);
}

#[test]
fn write_text_autosaves_without_versioning() {
    let (_tmp, lib) = setup();
    let a = lib.create_markdown_asset("", "笔记").unwrap();
    let base_hash = a.current_hash.clone();
    assert_eq!(a.file_name, "笔记.md");
    assert_eq!(a.ver_count, 1);

    // Two autosaves: content + title update, but no new version rows
    lib.write_asset_text(&a.id, "# 初稿\n\n一些内容").unwrap();
    let a2 = lib
        .write_asset_text(&a.id, "# 定稿\n\n更完整的内容")
        .unwrap();
    assert_eq!(a2.ver_count, 1);
    assert_eq!(a2.title, "定稿");

    // A scan sees the in-app write as already-indexed → no diff, no new version
    let sum = lib.scan(|_| {}).unwrap();
    assert!(!sum.changed());
    assert_eq!(lib.asset(&a.id).unwrap().ver_count, 1);

    // Ending the session checkpoints exactly one "编辑" version
    let ver = lib.checkpoint_version(&a.id, &base_hash).unwrap();
    assert_eq!(ver, Some(2));
    let vs = lib.list_versions(&a.id).unwrap();
    assert_eq!(vs.len(), 2);
    assert_eq!(vs[0].label, "编辑");

    // Checkpointing again with the same base is idempotent (dedup guard)
    assert_eq!(lib.checkpoint_version(&a.id, &base_hash).unwrap(), Some(2));
    assert_eq!(lib.list_versions(&a.id).unwrap().len(), 2);
}

#[test]
fn import_mixed_html_and_markdown() {
    let (tmp, lib) = setup();
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("a.html"), html("A", "内容甲")).unwrap();
    fs::write(outside.join("b.md"), md(None, "# B\n\n内容乙")).unwrap();
    fs::write(outside.join("c.txt"), "plain text").unwrap();

    let r = lib
        .import_files(
            &[
                outside.join("a.html"),
                outside.join("b.md"),
                outside.join("c.txt"),
            ],
            "",
        )
        .unwrap();
    assert_eq!(r.added, 2);
    assert_eq!(r.skipped, 1);
    assert!(harbly_core::is_managed_name("b.md"));
    assert!(!harbly_core::is_managed_name("c.txt"));
}

/// Build an hdoc document with a title heading and optional extra block markup.
fn hdoc(title: &str, extra: &str) -> String {
    format!(
        "<h-doc v=\"1\" theme=\"paper\">\n  <h1>{title}</h1>\n  <p>正文段落</p>\n{extra}</h-doc>\n"
    )
}

#[test]
fn hdoc_scan_title_attrs_and_fts() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(
        root.join("方案.hdoc"),
        hdoc(
            "发布方案",
            "  <h-callout kind=\"tip\" title=\"定价结论\"><p>按席位收费</p></h-callout>\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("blank.hdoc"),
        "<h-doc v=\"1\"><h1></h1><p>no heading text here</p></h-doc>",
    )
    .unwrap();

    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.added, 2);

    let all = lib.list_assets("", SortKey::Name).unwrap();
    let title_of = |name: &str| {
        all.iter()
            .find(|a| a.file_name == name)
            .unwrap()
            .title
            .clone()
    };
    assert_eq!(title_of("方案.hdoc"), "发布方案");
    assert_eq!(title_of("blank.hdoc"), "blank"); // empty <h1> → file stem

    // Component ATTRIBUTE text (callout title) is searchable via the jieba path
    let hits = lib.search("定价").unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset.file_name, "方案.hdoc");
    // Element text inside components is searchable too
    let hits = lib.search("席位").unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn hdoc_rename_preserves_extension() {
    let (_tmp, lib) = setup();
    fs::write(lib.root().join("page.hdoc"), hdoc("页", "")).unwrap();
    lib.scan(|_| {}).unwrap();
    let a = lib.list_assets("", SortKey::Recent).unwrap().remove(0);

    let r = lib.rename_asset(&a.id, "改名").unwrap();
    assert_eq!(r.file_name, "改名.hdoc");
    // Typing another managed extension never converts the type
    let r = lib.rename_asset(&a.id, "again.md").unwrap();
    assert_eq!(r.file_name, "again.hdoc");
    assert!(lib.rename_asset(&a.id, ".hdoc").is_err());
}

#[test]
fn hdoc_create_autosave_checkpoint_versions() {
    let (_tmp, lib) = setup();
    let a = lib.create_hdoc_asset("", "页面").unwrap();
    assert_eq!(a.file_name, "页面.hdoc");
    assert_eq!(a.ver_count, 1);
    assert_eq!(a.title, "页面"); // the skeleton's <h1> is empty → stem
    let disk = fs::read_to_string(lib.root().join("页面.hdoc")).unwrap();
    assert!(disk.contains("<h-doc v=\"1\""));

    let base = a.current_hash.clone();
    let a2 = lib
        .write_asset_text(
            &a.id,
            "<h-doc v=\"1\" theme=\"paper\">\n  <h1>路线图</h1>\n  <p>内容</p>\n</h-doc>\n",
        )
        .unwrap();
    assert_eq!(a2.title, "路线图");
    assert_eq!(a2.ver_count, 1); // autosave never versions

    // The in-app write is echo-suppressed: a scan sees no external change
    let sum = lib.scan(|_| {}).unwrap();
    assert!(!sum.changed());

    // Session end → one "编辑" version, snapshot carries the .hdoc extension
    assert_eq!(lib.checkpoint_version(&a.id, &base).unwrap(), Some(2));
    assert!(lib.version_file_path(&a.id, 2).ends_with("v2.hdoc"));

    lib.restore_version(&a.id, 1).unwrap();
    let restored = fs::read_to_string(lib.root().join("页面.hdoc")).unwrap();
    assert!(restored.contains("<h1></h1>"));
}

#[test]
fn import_hdoc_files() {
    let (tmp, lib) = setup();
    let outside = tmp.path().join("out");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("d.hdoc"), hdoc("导入页", "")).unwrap();
    let r = lib.import_files(&[outside.join("d.hdoc")], "").unwrap();
    assert_eq!(r.added, 1);
    assert!(harbly_core::is_managed_name("d.hdoc"));
}

#[test]
fn md_render_rewrites_relative_images() {
    // Relative images are rewritten to resolve through the asset protocol
    let out = harbly_core::md_to_html_body(
        "![alt](img/a.png)",
        Some("harbly-asset://localhost/rel/ID/"),
    );
    assert!(out.contains("harbly-asset://localhost/rel/ID/img/a.png"));
    // Absolute URLs are left untouched
    let out = harbly_core::md_to_html_body("![alt](https://example.com/a.png)", Some("base/"));
    assert!(out.contains("https://example.com/a.png"));
    // A space in the path (angle-bracket destination) is percent-encoded
    let out = harbly_core::md_to_html_body("![](<my pic.png>)", Some("b/"));
    assert!(out.contains("b/my%20pic.png"));
}

/// Finder tag interop: set_tags writes the xattr; Finder-side edits (color-number format) are adopted by scans; duplicates inherit tags
#[cfg(target_os = "macos")]
#[test]
fn finder_tags_xattr_interop() {
    let (_tmp, lib) = setup();
    let abs = lib.root().join("笔记.html");
    fs::write(&abs, html("笔记", "灵感内容")).unwrap();
    lib.scan(|_| {}).unwrap();
    let id = lib.list_assets("", SortKey::Name).unwrap()[0].id.clone();

    // Set in-app → lands in the file xattr (visible to Finder)
    lib.set_tags(&id, &["重要".into(), "灵感".into()]).unwrap();
    assert_eq!(
        harbly_core::read_file_tags(&abs),
        vec!["重要".to_string(), "灵感".to_string()]
    );

    // Simulate a Finder edit: write entries carrying a color number ("name\n6") → after a scan the database follows disk
    harbly_core::write_file_tags(&abs, &["红色标签\n6".into(), "工作".into()]).unwrap();
    let sum = lib.scan(|_| {}).unwrap();
    assert_eq!(sum.tags_synced, 1);
    assert!(sum.changed());
    let mut tags = lib.asset(&id).unwrap().tags;
    tags.sort();
    assert_eq!(tags, vec!["工作".to_string(), "红色标签".to_string()]);

    // A second scan with no diff → not counted as a change (watcher echo suppression relies on this)
    let sum2 = lib.scan(|_| {}).unwrap();
    assert_eq!(sum2.tags_synced, 0);
    assert!(!sum2.changed());

    // The duplicate inherits tags (same behavior as Finder), with the color suffix preserved verbatim
    let dup = lib.duplicate_asset(&id).unwrap();
    let mut dup_tags = dup.tags.clone();
    dup_tags.sort();
    assert_eq!(dup_tags, vec!["工作".to_string(), "红色标签".to_string()]);

    // Clearing = removing the attribute
    lib.set_tags(&id, &[]).unwrap();
    assert!(harbly_core::read_file_tags(&abs).is_empty());
}

#[test]
fn ai_apply_output_appends_version_and_reindexes() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(root.join("page.html"), html("Pricing", "旧的定价方案")).unwrap();
    lib.scan(|_| {}).unwrap();
    let a = lib.asset_by_rel("page.html").unwrap();
    assert_eq!(a.ver_count, 1);

    let ver = lib
        .apply_ai_output(&a.id, &html("Pricing", "全新的深色定价"), "AI 改版")
        .unwrap();
    assert_eq!(ver, 2);

    // File content, version chain, and FTS index all reflect the new content
    let a2 = lib.asset(&a.id).unwrap();
    assert_eq!(a2.ver_count, 2);
    let versions = lib.list_versions(&a.id).unwrap();
    assert_eq!(versions[0].label, "AI 改版");
    assert!(lib.read_asset_text(&a.id).unwrap().contains("深色定价"));
    assert_eq!(lib.search("深色").unwrap().len(), 1);

    // Identical content again → dedup guard keeps the chain unchanged
    let same = lib
        .apply_ai_output(&a.id, &html("Pricing", "全新的深色定价"), "AI 改版")
        .unwrap();
    assert_eq!(same, 2);
    assert_eq!(lib.asset(&a.id).unwrap().ver_count, 2);

    // Rollback restores v1 content as a new version (history never rewritten)
    lib.restore_version(&a.id, 1).unwrap();
    assert!(lib.read_asset_text(&a.id).unwrap().contains("旧的定价方案"));
    assert_eq!(lib.asset(&a.id).unwrap().ver_count, 3);
}

#[test]
fn ai_write_snapshots_unversioned_live_edits_first() {
    let (_tmp, lib) = setup();
    fs::write(lib.root().join("note.html"), html("Note", "第一版")).unwrap();
    lib.scan(|_| {}).unwrap();
    let a = lib.asset_by_rel("note.html").unwrap();

    // Editor autosave: live file changes with NO version captured
    lib.write_asset_text(&a.id, &html("Note", "用户手打的心血内容"))
        .unwrap();
    assert_eq!(lib.asset(&a.id).unwrap().ver_count, 1);

    // An AI write must not destroy the only copy of those edits: the live
    // content is checkpointed (编辑) before the AI version lands
    let ver = lib
        .apply_ai_output(&a.id, &html("Note", "AI 重写后的内容"), "AI 改版")
        .unwrap();
    assert_eq!(ver, 3);
    let versions = lib.list_versions(&a.id).unwrap();
    assert_eq!(versions[0].label, "AI 改版");
    assert_eq!(versions[1].label, "编辑");
    let saved = fs::read_to_string(lib.version_file_path(&a.id, 2)).unwrap();
    assert!(saved.contains("用户手打的心血内容"));

    // Even a byte-identical AI re-emission of an OLD version cannot clobber
    // newer live edits: the pre-snapshot bumps the chain first
    lib.write_asset_text(&a.id, &html("Note", "又一轮未存档编辑"))
        .unwrap();
    let ver = lib
        .apply_ai_output(&a.id, &html("Note", "AI 重写后的内容"), "AI 改版")
        .unwrap();
    assert_eq!(ver, 5);
    let recovered = fs::read_to_string(lib.version_file_path(&a.id, 4)).unwrap();
    assert!(recovered.contains("又一轮未存档编辑"));
}

#[test]
fn ai_runs_record_list_and_cleanup() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(root.join("a.html"), html("A", "内容")).unwrap();
    lib.scan(|_| {}).unwrap();
    let a = lib.asset_by_rel("a.html").unwrap();

    let rec = lib
        .record_ai_run(&harbly_core::NewAiRun {
            asset_id: a.id.clone(),
            kind: "revise".into(),
            supply: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            instruction: "改成深色".into(),
            status: "ok".into(),
            ver: Some(2),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rec.status, "ok");
    lib.record_ai_run(&harbly_core::NewAiRun {
        asset_id: a.id.clone(),
        kind: "review".into(),
        supply: "claude".into(),
        model: String::new(),
        instruction: String::new(),
        status: "ok".into(),
        ver: None,
        report: Some("# 报告\n一切正常".into()),
        ..Default::default()
    })
    .unwrap();

    let runs = lib.list_ai_runs(&a.id, 50).unwrap();
    assert_eq!(runs.len(), 2);
    // Newest first: the review comes back on top
    assert_eq!(runs[0].kind, "review");
    assert_eq!(runs[0].report.as_deref(), Some("# 报告\n一切正常"));
    assert_eq!(runs[1].ver, Some(2));

    // Trashing the asset clears its run records too
    lib.trash_asset(&a.id).unwrap();
    assert!(lib.list_ai_runs(&a.id, 50).unwrap().is_empty());
}

#[test]
fn ai_sessions_and_messages_roundtrip() {
    let (_tmp, lib) = setup();

    let s = lib.create_ai_session("claude", "", "medium").unwrap();
    assert_eq!(s.title, "");

    // First user message titles the session; appends bump activity order
    lib.append_ai_message(&s.id, "user", "帮我完善这个文档，第一段更口语化", &[])
        .unwrap();
    lib.append_ai_message(
        &s.id,
        "assistant",
        "已完善。",
        &["read_asset a.html".to_string()],
    )
    .unwrap();
    let msgs = lib.list_ai_messages(&s.id).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[1].actions, vec!["read_asset a.html".to_string()]);

    let got = lib.get_ai_session(&s.id).unwrap().unwrap();
    assert!(got.title.starts_with("帮我完善"));

    // A second session becomes the most recent one
    let s2 = lib
        .create_ai_session("anthropic", "claude-sonnet-5", "high")
        .unwrap();
    let list = lib.list_ai_sessions(10).unwrap();
    assert_eq!(list[0].id, s2.id);

    // Prefs update; switching supply drops the foreign resume id
    lib.set_ai_session_agent_id(&s.id, "abc-123").unwrap();
    lib.set_ai_session_prefs(&s.id, "claude", "", "high")
        .unwrap();
    assert_eq!(
        lib.get_ai_session(&s.id).unwrap().unwrap().agent_session_id,
        Some("abc-123".to_string())
    );
    lib.set_ai_session_prefs(&s.id, "openai", "gpt-5.1", "")
        .unwrap();
    assert_eq!(
        lib.get_ai_session(&s.id).unwrap().unwrap().agent_session_id,
        None
    );

    // Delete hands back a snapshot; restore resurrects transcript + title
    let snap = lib.delete_ai_session(&s.id).unwrap().unwrap();
    assert!(lib.get_ai_session(&s.id).unwrap().is_none());
    assert!(lib.list_ai_messages(&s.id).unwrap().is_empty());
    assert_eq!(snap.messages.len(), 2);
    lib.restore_ai_session(&snap).unwrap();
    let back = lib.get_ai_session(&s.id).unwrap().unwrap();
    assert!(back.title.starts_with("帮我完善"));
    let msgs = lib.list_ai_messages(&s.id).unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[1].actions, vec!["read_asset a.html".to_string()]);
    // Deleting a nonexistent session is a no-op, not an error
    assert!(lib.delete_ai_session("ghost").unwrap().is_none());
}

#[test]
fn ai_creates_new_asset() {
    let (_tmp, lib) = setup();
    let a = lib
        .create_asset_from_ai(
            "",
            "合并结果",
            "<!doctype html><html><title>合并</title></html>",
        )
        .unwrap();
    assert_eq!(a.file_name, "合并结果.html");
    assert_eq!(a.source, "ai");
    assert_eq!(a.ver_count, 1);
    // Name collision auto-suffixes; explicit .md keeps its type
    let b = lib
        .create_asset_from_ai("", "合并结果", "<html>2</html>")
        .unwrap();
    assert_eq!(b.file_name, "合并结果-2.html");
    let c = lib
        .create_asset_from_ai("笔记", "notes.md", "# hi")
        .unwrap();
    assert_eq!(c.rel_path, "笔记/notes.md");
    assert_eq!(lib.search("合并").unwrap().len(), 2);
}

#[test]
fn ai_tool_surface_reads_writes_and_records() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(root.join("pricing.html"), html("定价", "旧定价方案")).unwrap();
    lib.scan(|_| {}).unwrap();
    let a = lib.asset_by_rel("pricing.html").unwrap();
    let s = lib
        .create_ai_session("anthropic", "claude-sonnet-5", "")
        .unwrap();
    let ctx = harbly_core::AiToolCtx {
        supply: "anthropic".into(),
        model: "claude-sonnet-5".into(),
        session_id: Some(s.id.clone()),
    };

    // search → read → write → create, all through the single tool entry point
    let (v, w) = lib
        .execute_ai_tool("search_library", &serde_json::json!({"query":"定价"}), &ctx)
        .unwrap();
    assert!(w.is_none());
    assert_eq!(v["results"][0]["asset_id"], a.id.as_str());

    let (v, _) = lib
        .execute_ai_tool("read_asset", &serde_json::json!({"asset_id": a.id}), &ctx)
        .unwrap();
    assert!(v["content"].as_str().unwrap().contains("旧定价方案"));

    let (v, w) = lib
        .execute_ai_tool(
            "write_asset",
            &serde_json::json!({"asset_id": a.id, "content": html("定价", "新深色定价"), "summary": "换深色"}),
            &ctx,
        )
        .unwrap();
    assert_eq!(v["ver"], 2);
    assert!(!w.as_ref().unwrap().created);
    assert!(lib.read_asset_text(&a.id).unwrap().contains("新深色定价"));

    let (v, w) = lib
        .execute_ai_tool(
            "create_asset",
            &serde_json::json!({"name": "merged", "content": "<html><title>合并页</title></html>", "folder": ""}),
            &ctx,
        )
        .unwrap();
    assert_eq!(v["file_name"], "merged.html");
    assert!(w.unwrap().created);

    // Both writes recorded with session attribution; message link-up works
    let runs = lib.list_ai_runs(&a.id, 10).unwrap();
    assert_eq!(runs[0].kind, "revise");
    assert_eq!(runs[0].session_id.as_deref(), Some(s.id.as_str()));
    assert!(runs[0].message_id.is_none());
    lib.link_runs_to_message(&s.id, "msg-1", 0).unwrap();
    let runs = lib.list_ai_runs(&a.id, 10).unwrap();
    assert_eq!(runs[0].message_id.as_deref(), Some("msg-1"));

    // Guard rails: unknown tool, bad ids, traversal
    assert!(lib
        .execute_ai_tool("rm_rf", &serde_json::json!({}), &ctx)
        .is_err());
    assert!(lib
        .execute_ai_tool("read_asset", &serde_json::json!({"asset_id":"nope"}), &ctx)
        .is_err());
    assert!(lib
        .execute_ai_tool(
            "create_asset",
            &serde_json::json!({"name":"x","content":"y","folder":"../out"}),
            &ctx
        )
        .is_err());
}

#[test]
fn ai_tools_list_and_delete() {
    let (_tmp, lib) = setup();
    let root = lib.root().to_path_buf();
    fs::write(root.join("full.html"), html("Full", "有内容")).unwrap();
    fs::write(root.join("empty.html"), "").unwrap();
    lib.scan(|_| {}).unwrap();
    let ctx = harbly_core::AiToolCtx::default();

    // Enumeration exposes sizes — this is how "find the empty files" works
    let (v, w) = lib
        .execute_ai_tool("list_assets", &serde_json::json!({}), &ctx)
        .unwrap();
    assert!(w.is_none());
    assert_eq!(v["total"], 2);
    let assets = v["assets"].as_array().unwrap();
    let empty = assets
        .iter()
        .find(|a| a["file_name"] == "empty.html")
        .unwrap();
    assert_eq!(empty["size_bytes"], 0);
    let empty_id = empty["asset_id"].as_str().unwrap().to_string();

    // Deletion goes to the system Trash and cleans the index
    let (v, w) = lib
        .execute_ai_tool(
            "delete_asset",
            &serde_json::json!({ "asset_id": empty_id }),
            &ctx,
        )
        .unwrap();
    assert_eq!(v["deleted"], "empty.html");
    assert!(!w.unwrap().created);
    assert!(lib.asset(&empty_id).is_err());
    assert_eq!(lib.total_count().unwrap(), 1);

    // Folder traversal rejected on list too
    assert!(lib
        .execute_ai_tool("list_assets", &serde_json::json!({"folder": "../x"}), &ctx)
        .is_err());

    // Absolute folders must not escape the library (Path::join swaps the root)
    let outside = std::env::temp_dir().join("harbly-escape-test");
    let _ = fs::remove_dir_all(&outside);
    assert!(lib
        .execute_ai_tool(
            "create_asset",
            &serde_json::json!({
                "folder": outside.to_string_lossy(),
                "name": "evil",
                "content": "<html>x</html>",
            }),
            &ctx,
        )
        .is_err());
    assert!(!outside.exists());
    assert!(lib
        .execute_ai_tool("list_assets", &serde_json::json!({"folder": "/etc"}), &ctx)
        .is_err());
}
