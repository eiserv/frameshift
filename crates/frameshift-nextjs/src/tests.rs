use super::detect::{ConfigLoad, Router, RouterDirectory, RouterState};

#[test]
fn router_kinds_follow_route_files() {
    let directory = |routes| RouterDirectory {
        path: frameshift_core::SourcePath::new("app", frameshift_core::PathLimits::default())
            .unwrap(),
        in_src: false,
        state: RouterState::Directory {
            routes,
            complete: true,
        },
    };
    let detection = |app, pages| super::Detection {
        verdict: super::Verdict::NextJs,
        dependency: None,
        version: super::VersionEvidence::Unknown,
        config: None,
        app,
        pages,
    };
    assert_eq!(detection(Some(directory(2)), None).router(), Router::App);
    assert_eq!(detection(None, Some(directory(1))).router(), Router::Pages);
    assert_eq!(
        detection(Some(directory(1)), Some(directory(1))).router(),
        Router::Hybrid
    );
    assert_eq!(detection(Some(directory(0)), None).router(), Router::None);
    let _ = ConfigLoad::Loaded;
}
