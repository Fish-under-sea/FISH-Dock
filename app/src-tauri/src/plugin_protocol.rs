//! JSON 行协议；单条消息有界，响应必须匹配当前请求。
use std::io::{BufRead, Read};
use serde_json::Value;
pub const MAX_LINE:u64=1024*1024;
// 插件没说清是什么错的时候回给前端的固定提示：业务错误，插件进程仍可继续。
pub const OPERATION_ERROR:&str="插件操作未完成，请检查配置后重试";
// 业务错误的码要一路透传到页面（#13-1 / #14-4），编码成「<文案>（错误码 <CODE>）」：
// 给人看仍是可读文案（设置页的失败记录照旧可读），给程序看时尾部能取回码；
// CODE 只允许 ASCII 大写字母、数字与下划线，与中文文案不会混淆。
const CODE_TAIL:&str="（错误码 ";
const CODE_MAX:usize=40;
// 与 sdk/node/plugin.mjs 的上限对齐：那边按字符算（≤200），一个字符最多 3 字节，
// 这里放宽到 600 字节，正常业务文案放行、纯粹的超长原文仍然拒绝。
const MESSAGE_MAX:usize=600;
fn code_valid(code:&str)->bool{
    let bytes=code.as_bytes();
    !bytes.is_empty()&&bytes.len()<=CODE_MAX&&bytes[0].is_ascii_uppercase()
        &&bytes.iter().all(|byte|byte.is_ascii_uppercase()||byte.is_ascii_digit()||*byte==b'_')
}
fn message_valid(message:&str)->bool{
    !message.is_empty()&&message.len()<=MESSAGE_MAX&&!message.chars().any(char::is_control)
}
pub fn business_error(code:&str,message:&str)->String{format!("{message}{CODE_TAIL}{code}）")}
// 取回 {code, message}；宿主自己的固定提示（OPERATION_ERROR 等）返回 None。
pub fn split_error(error:&str)->Option<(&str,&str)>{
    let tail=error.rfind(CODE_TAIL)?;
    let code=error[tail+CODE_TAIL.len()..].strip_suffix('）')?;
    if tail==0||!code_valid(code){return None}
    Some((code,&error[..tail]))
}
// 业务错误：插件活着，只是这次调用没成，进程不该被回收。
pub fn is_business_error(error:&str)->bool{split_error(error).is_some()}
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
        // 结构坏掉才是致命错误（协议本身不可信）；码或文案不可用时仍按业务错误处理。
        if value.get("result").is_some()||!error.is_object(){return Err("插件错误响应结构无效".into());}
        let code=error.get("code").and_then(Value::as_str).unwrap_or("");
        let message=error.get("message").and_then(Value::as_str).unwrap_or("");
        if code_valid(code)&&message_valid(message) { return Err(business_error(code,message)); }
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
        assert_eq!(answer(valid.clone(),9).unwrap_err(),business_error("COMMAND_FAILED","synthetic validation"));
        assert_ne!(answer(valid,8).unwrap_err(),OPERATION_ERROR);
        for invalid in [serde_json::json!({"id":9,"error":"broken"}),serde_json::json!({"id":9,"result":null,"error":{"code":"X","message":"conflicting"}})]{assert_ne!(answer(invalid,9).unwrap_err(),OPERATION_ERROR);}
    }
    // 插件业务错误的码必须原样上行，页面才查得到专属文案；尾部编码要能原样取回。
    #[test]fn business_errors_carry_their_code_upward(){
        let encoded=answer(serde_json::json!({"id":7,"error":{"code":"PORT_IN_USE","message":"端口已被占用，请停止冲突服务或更换本机端口后重试"}}),7).unwrap_err();
        assert_eq!(encoded,"端口已被占用，请停止冲突服务或更换本机端口后重试（错误码 PORT_IN_USE）");
        assert!(is_business_error(&encoded));
        assert_eq!(split_error(&encoded),Some(("PORT_IN_USE","端口已被占用，请停止冲突服务或更换本机端口后重试")));
        // 宿主自己的固定提示不是业务错误，不能被当成可解码的码。
        assert!(!is_business_error(OPERATION_ERROR));
        assert!(!is_business_error("插件响应 ID 不匹配"));
        assert_eq!(split_error("端口已被占用（错误码 lower case）"),None);
        assert_eq!(split_error("（错误码 AB）"),None);
        // 码或文案不可用时退回固定提示，不把不可信的码拼进去。
        for broken in [serde_json::json!({"id":7,"error":{"code":"lower case","message":"x"}}),
            serde_json::json!({"id":7,"error":{"code":"OK"}}),
            serde_json::json!({"id":7,"error":{"code":"OK","message":""}}),
            serde_json::json!({"id":7,"error":{"code":"OK","message":"line\nbreak"}})]{
            assert_eq!(answer(broken,7).unwrap_err(),OPERATION_ERROR);
        }
        // 长文案被拒，避免把插件的原始长文本（可能夹带地址）透给宿主；边界内仍放行。
        let long=serde_json::json!({"id":7,"error":{"code":"OK","message":"x".repeat(MESSAGE_MAX+1)}});
        assert_eq!(answer(long,7).unwrap_err(),OPERATION_ERROR);
        let edge=serde_json::json!({"id":7,"error":{"code":"OK","message":"x".repeat(MESSAGE_MAX)}});
        assert!(is_business_error(&answer(edge,7).unwrap_err()));
        // 插件仓最长的固定文案（SETTINGS_INVALID，155 字节）必须在边界内，否则码会被静默丢掉。
        let longest=serde_json::json!({"id":7,"error":{"code":"SETTINGS_INVALID","message":"Magpie 设置文件已损坏（原文件已保留），请检查插件资料下 credentials/magpie-runtime/data/settings.json，修复配置后重新启动"}});
        assert!(is_business_error(&answer(longest,7).unwrap_err()));
    }
    // 文案本身就带括号、甚至带同样的「错误码」字样时，只认最后一段尾巴。
    #[test]fn error_text_round_trips_through_the_page_tail(){
        let encoded=business_error("PROXY_REGISTRY_WRITE_FAILED","无法写入系统代理设置");
        assert_eq!(split_error(&encoded),Some(("PROXY_REGISTRY_WRITE_FAILED","无法写入系统代理设置")));
        let tricky=business_error("EACCES","没有访问权限（EACCES），先别慌");
        assert_eq!(split_error(&tricky),Some(("EACCES","没有访问权限（EACCES），先别慌")));
    }
}