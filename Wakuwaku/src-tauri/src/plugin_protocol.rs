//! JSON 行协议；单条消息有界，响应必须匹配当前请求。
use std::io::{BufRead, Read};
use serde_json::Value;
pub const MAX_LINE:u64=1024*1024;
// 完整编号匹配的业务错误已消费响应，后续请求仍可继续。
pub const OPERATION_ERROR:&str="插件操作未完成，请检查配置后重试";
pub fn line(reader:&mut impl BufRead)->Result<Option<Value>,String>{
    let mut bytes=Vec::new();
    let n=reader.take(MAX_LINE+1).read_until(b'\n',&mut bytes).map_err(|_|"插件协议读取失败")?;
    if n==0 { return Ok(None); }
    if n as u64>MAX_LINE || bytes.last()!=Some(&b'\n') { return Err("插件协议行超过限制或未完整结束".into()); }
    let value:Value=serde_json::from_slice(&bytes).map_err(|_|"插件协议消息不是有效 JSON")?;
    if !value.is_object() || value.get("id").is_none_or(|id| !id.is_number() && !id.is_string()) { return Err("插件协议缺少有效请求 ID".into()); }
    Ok(Some(value))
}
pub fn answer(value:Value,id:u64)->Result<Value,String>{
    if value["id"].as_u64()!=Some(id) { return Err("插件响应 ID 不匹配".into()); }
    if let Some(error)=value.get("error") {
        if value.get("result").is_some()||error.get("code").and_then(Value::as_str).is_none()||error.get("message").and_then(Value::as_str).is_none(){return Err("插件错误响应结构无效".into());}
        return Err(OPERATION_ERROR.into());
    }
    value.get("result").cloned().ok_or("插件响应缺少结果".into())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn rejects_partial_oversized_and_mismatched_messages(){
        for bytes in [b"{\"id\":1}".to_vec(),vec![b'a';MAX_LINE as usize+1],b"[]\n".to_vec(),b"{\"method\":\"x\"}\n".to_vec()]{assert!(line(&mut bytes.as_slice()).is_err());}
        let value=line(&mut b"{\"id\":9,\"result\":{\"ready\":true}}\n".as_slice()).unwrap().unwrap();
        assert!(answer(value.clone(),8).is_err());assert_eq!(answer(value,9).unwrap()["ready"],true);
        let valid=serde_json::json!({"id":9,"error":{"code":"COMMAND_FAILED","message":"synthetic validation"}});
        assert_eq!(answer(valid.clone(),9).unwrap_err(),OPERATION_ERROR);
        assert_ne!(answer(valid,8).unwrap_err(),OPERATION_ERROR);
        for invalid in [serde_json::json!({"id":9,"error":"broken"}),serde_json::json!({"id":9,"result":null,"error":{"code":"X","message":"conflicting"}})]{assert_ne!(answer(invalid,9).unwrap_err(),OPERATION_ERROR);}
    }
}
