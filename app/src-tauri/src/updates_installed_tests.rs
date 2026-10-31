use super::*;
#[test]
fn installed_build_checks_releases_but_never_swaps_a_raw_exe(){
 for from_source in [true,false]{for forced in [true,false]{let mode=update_mode(true,from_source,forced);assert!(mode.installed);assert!(!mode.source);assert!(mode.may_check);assert!(!mode.may_replace);}}
 let dev=update_mode(false,true,false);assert!(dev.source);assert!(!dev.may_check);assert!(!dev.may_replace);
 let binary=update_mode(false,false,false);assert!(binary.may_check&&binary.may_replace);
}
#[test]
fn api_403_falls_back_to_official_release_without_claiming_a_verified_payload(){
 let result=github_sources(||Err("HTTP 403 rate limit".into()),||Ok(public_release_url("https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0"))).unwrap().unwrap();
 assert_eq!(result.version,"1.2.0");assert!(result.digest.is_none());assert!(result.exe.is_none());
 assert!(!merge(vec![result]).unwrap().can_install());
}
#[test]
fn fallback_accepts_current_api_results_and_reports_both_failures(){
 let mut used=false;
 let expected=public_release_url("https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.3.0");
 assert_eq!(github_sources(||Ok(expected.clone()),||{used=true;Err("must not fetch".into())}).unwrap(),expected);assert!(!used);
 let error=github_sources(||Err("api unavailable".into()),||Err("release page unavailable".into())).unwrap_err();assert!(error.contains("api unavailable"));assert!(error.contains("release page unavailable"));
}
#[test]
fn release_page_redirects_are_bound_to_the_official_repository_and_stable_versions(){
 for bad in ["http://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0","https://github.com.evil.test/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0","https://github.com/another/repository/releases/tag/v1.2.0","https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0-beta","https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0/extra","https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0?payload=evil"]{assert!(public_release_url(bad).is_none(),"{bad}");}
 assert_eq!(public_release_url("https://github.com/Fish-under-sea/FISH-Dock/releases/tag/v1.2.0").unwrap().version,"1.2.0");
}
#[test]
#[ignore="显式运行以检查未登录公开 GitHub API 或官方 Release 页面；不使用账号凭据"]
fn live_official_release_check_without_credentials(){
 let got=fetch(Source::GitHub,GITHUB_LATEST).unwrap().unwrap();println!("Official release: version={}, page={}, payloadDigest={}",got.version,got.page,got.digest.is_some());assert!(got.page.starts_with(GITHUB_REPO));crate::plugin_manifest::version(&got.version).unwrap();
}
