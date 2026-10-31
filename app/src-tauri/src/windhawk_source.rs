//! Windhawk 源 mod 的元数据与集合校验；不会把待编译源码报告为已安装。
use serde::{Deserialize,Serialize};

#[derive(Clone,Debug,Deserialize,Serialize,PartialEq)]
#[serde(rename_all="camelCase")]
pub struct Metadata { pub id:String,pub name:String,pub author:String,pub version:String,pub description:String,pub include:Vec<String>,pub exclude:Vec<String> }
pub fn valid_id(id:&str)->bool { !id.is_empty() && id.len()<=128 && id.bytes().all(|c|c.is_ascii_alphanumeric()||matches!(c,b'-'|b'_'|b'@')) }
pub fn metadata(source:&str)->Result<Metadata,String>{
 if source.len()>2*1024*1024 || source.contains('\0'){return Err("mod 源文件超过 2 MiB 或含空字符".into());}
 let mut started=false;let mut ended=false;let mut values=std::collections::BTreeMap::<String,Vec<String>>::new();
 for line in source.lines(){
  let line=line.trim().trim_start_matches('\u{feff}');
  if line=="// ==WindhawkMod==" {if started{return Err("重复的 mod 元数据头".into());}started=true;continue;}
  if line=="// ==/WindhawkMod==" {ended=true;break;}
  if !started{continue;}
  let Some(row)=line.strip_prefix("//").map(str::trim).and_then(|s|s.strip_prefix('@')) else{continue};
  let Some(at)=row.find(char::is_whitespace) else{continue};
  let (key,value)=row.split_at(at);let value=value.trim();
  if !value.is_empty(){values.entry(key.into()).or_default().push(value.into());}
 }
 if !started||!ended{return Err("缺少完整 Windhawk 元数据头".into());}
 let field=|key:&str,required:bool|->Result<String,String>{match values.get(key){Some(v) if v.len()==1=>Ok(v[0].clone()),None if !required=>Ok(String::new()),_=>Err(format!("mod 字段缺失或重复：{key}"))}};
 let id=field("id",true)?;if !valid_id(&id){return Err("mod ID 含无效字符".into());}
 Ok(Metadata{id,name:field("name",true)?,author:field("author",false)?,version:field("version",true)?,description:field("description",false)?,include:values.remove("include").unwrap_or_default(),exclude:values.remove("exclude").unwrap_or_default()})
}

pub struct Compilation { pub architecture:Vec<String>,pub options:Vec<String>,pub settings:std::collections::BTreeMap<String,serde_json::Value> }
pub fn compilation(source:&str)->Result<Compilation,String>{
 metadata(source)?;let mut architecture=Vec::new();let mut options=Vec::new();let mut header=false;
 for line in source.lines(){let line=line.trim().trim_start_matches('\u{feff}');if line=="// ==WindhawkMod=="{header=true;continue;}if line=="// ==/WindhawkMod=="{break;}if !header{continue;}
  if let Some(value)=line.strip_prefix("//").map(str::trim).and_then(|s|s.strip_prefix("@architecture ")){for value in value.split_whitespace(){if !["x86","x86-64","amd64","arm64"].contains(&value){return Err(format!("编译架构不受支持：{value}"));}if !architecture.contains(&value.to_string()){architecture.push(value.into());}}}
  if let Some(value)=line.strip_prefix("//").map(str::trim).and_then(|s|s.strip_prefix("@compilerOptions ")){options.extend(shlex::split(value).ok_or("编译选项引号未闭合")?);}
 }
 let mut settings=std::collections::BTreeMap::new();
 if let Some((_,tail))=source.split_once("// ==WindhawkModSettings=="){
  let (body,_)=tail.split_once("// ==/WindhawkModSettings==").ok_or("设置块未闭合")?;
  let body=body.trim().strip_prefix("/*").and_then(|v|v.trim().strip_suffix("*/")).ok_or("设置应放在块注释内")?;
  let value:serde_yaml::Value=serde_yaml::from_str(body).map_err(|e|format!("设置 YAML 无效：{e}"))?;
  flatten_settings("",&value,&mut settings,0)?;
 }
 Ok(Compilation{architecture,options,settings})
}
fn flatten_settings(prefix:&str,value:&serde_yaml::Value,out:&mut std::collections::BTreeMap<String,serde_json::Value>,depth:usize)->Result<(),String>{
 use serde_yaml::Value as Y;
 if depth>32||out.len()>4096{return Err("设置嵌套或数量超出限制".into());}
 match value{
 Y::Mapping(map)=>{for(key,value)in map{let key=key.as_str().ok_or("设置名称必须为字符串")?;if key.starts_with('$'){continue;}if key.is_empty()||key.contains(['\r','\n','\0','=','[',']']){return Err("设置键名无效".into());}let name=if prefix.is_empty(){key.into()}else{format!("{prefix}.{key}")};flatten_settings(&name,value,out,depth+1)?;}},
 Y::Sequence(items)=>{for(i,item)in items.iter().enumerate(){let name=if item.is_mapping(){prefix.to_string()}else{format!("{prefix}[{i}]")};flatten_settings(&name,item,out,depth+1)?;}},
 Y::String(s)=>{if s.contains(['\0','\r','\n']){return Err("设置字符串含控制字符".into());}if out.insert(prefix.into(),serde_json::json!(s)).is_some(){return Err("重复设置键".into());}},
 Y::Number(n)=>{let n=n.as_i64().filter(|v|*v>=i32::MIN as i64&&*v<=u32::MAX as i64).ok_or("设置数值必须为 32 位整数")?;if out.insert(prefix.into(),serde_json::json!(n)).is_some(){return Err("重复设置键".into());}},
 _=>return Err("设置默认值应为字符串、整数或分组".into())
 }Ok(())
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn compilation_preserves_architecture_options_and_nested_settings(){
   let source="// ==WindhawkMod==\n// @id test-mod\n// @name 测试\n// @version 1.0\n// @architecture x86-64\n// @compilerOptions -lcomctl32 -DNAME=\"hello world\"\n// ==/WindhawkMod==\n// ==WindhawkModSettings==\n/*\n- count: 4\n- color: '#ffffff'\n- group:\n  - enabled: 1\n  - names: [first, second]\n*/\n// ==/WindhawkModSettings==";
   let info=compilation(source).unwrap();assert_eq!(info.architecture,vec!["x86-64"]);assert!(info.options.contains(&"-DNAME=hello world".to_string()));
   assert_eq!(info.settings.get("count"),Some(&serde_json::json!(4)));assert_eq!(info.settings.get("group.names[1]"),Some(&serde_json::json!("second")));
   assert!(compilation(&source.replace("x86-64","unknown-arch")).is_err());
  }
  #[test] fn parses_windhawk_header_without_interpreting_cpp(){
  let text="// ==WindhawkMod==\n// @id test-mod\n// @name 示例模组\n// @author Fish\n// @version 1.2.3\n// @description 演示\n// @include explorer.exe\n// @include notepad.exe\n// ==/WindhawkMod==\nvoid Wh_ModInit(){}";
  let got=metadata(text).unwrap(); assert_eq!(got.id,"test-mod");assert_eq!(got.name,"示例模组");assert_eq!(got.include,vec!["explorer.exe","notepad.exe"]);
  assert!(metadata(&text.replace("test-mod","../escape")).is_err());
  assert!(metadata(&text.replace("// @version 1.2.3\n","")).is_err());
  assert!(metadata(&text.replace("// @id test-mod","// @id test-mod\n// @id other")).is_err());
  assert!(metadata("// @id pretend\n// @name outside").is_err());
 }
}
