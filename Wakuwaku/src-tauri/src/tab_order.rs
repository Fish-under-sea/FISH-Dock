//! 外观标签排序契约，与页面稳定 ID 及有界历史一致。
fn segment(value:&str)->bool { !value.is_empty() && value.split('-').all(|part| !part.is_empty() && part.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit())) }
pub fn page_id(id:&str)->bool {
    if ["now","agents","look","widgets","mail"].contains(&id){return true;}
    if id.len()>160 {return false;}
    let Some((plugin,page))=id.split_once('/')else{return false};
    let Some((publisher,name))=plugin.split_once('.')else{return false};
    segment(publisher)&&segment(name)&&!page.is_empty()&&page.len()<=64&&page.bytes().all(|b|b.is_ascii_lowercase()||b.is_ascii_digit()||b==b'-')
}
pub fn valid(value:&serde_json::Value)->bool {
    let Some(object)=value.as_object()else{return false};
    if object.len()!=1{return false;}
    let Some(ids)=object.get("tabOrder").and_then(serde_json::Value::as_array)else{return false};
    let mut seen=std::collections::HashSet::new();
    ids.len()<=128&&ids.iter().all(|id|id.as_str().is_some_and(|s|page_id(s)&&seen.insert(s)))
}
#[cfg(test)]mod tests {
    use super::*;use serde_json::json;
    #[test]fn appearance_accepts_only_unique_bounded_page_ids(){
        assert!(valid(&json!({"tabOrder":["look","fish.ofm/console","now"]})));
        assert!(valid(&json!({"tabOrder":[]})));
        for bad in [json!({"tabOrder":["look","look"]}),json!({"tabOrder":["../escape"]}),json!({"tabOrder":["models"]}),json!({"tabOrder":["fish.ofm/a/b"]}),json!({"tabOrder":[],"secret":"hidden"}),json!({"tabOrder":["A.b/page"]})] {assert!(!valid(&bad),"{bad}");}
        let too_many:Vec<_>=(0..129).map(|i|format!("acme.test/page-{i}")).collect();assert!(!valid(&json!({"tabOrder":too_many})));
    }
}
