//! Windows 文件选择与系统下载目录；只返回用户确认的路径，不落地临时身份内容。
use std::path::{Path, PathBuf};

pub fn export_directory(configured: Option<&str>) -> Result<PathBuf, String> {
    let directory = configured.filter(|s| !s.trim().is_empty()).map(PathBuf::from).map(Ok).unwrap_or_else(|| {
        crate::storage_context::current().map(|context| Ok(context.host.path().join("exports"))).unwrap_or_else(downloads)
    })?;
    if !directory.is_absolute() { return Err("导出目录必须是绝对路径".into()); }
    crate::identity_store::safe(&directory)?;
    if !directory.is_dir() { return Err("导出目录不存在，请选择已有目录".into()); }
    Ok(directory)
}

#[cfg(windows)]
pub fn downloads() -> Result<PathBuf, String> {
    #[repr(C)] struct Guid { a: u32, b: u16, c: u16, d: [u8;8] }
    const DOWNLOADS: Guid = Guid { a:0x374de290,b:0x123f,c:0x4565,d:[0x91,0x64,0x39,0xc4,0x92,0x5e,0x46,0x7b] };
    #[link(name="shell32")] unsafe extern "system" { fn SHGetKnownFolderPath(id:*const Guid,flags:u32,token:isize,path:*mut *mut u16)->i32; }
    #[link(name="ole32")] unsafe extern "system" { fn CoTaskMemFree(memory:*mut std::ffi::c_void); }
    let mut path = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&DOWNLOADS,0,0,&mut path) } < 0 || path.is_null() { return Err("无法读取系统下载目录，请手动填写导出位置".into()); }
    let mut n = 0usize;
    unsafe { while n < 32768 && *path.add(n) != 0 { n+=1; } }
    let text = unsafe { String::from_utf16(std::slice::from_raw_parts(path,n)) };
    unsafe { CoTaskMemFree(path.cast()); }
    text.map(PathBuf::from).map_err(|_| "系统下载目录不是有效 Unicode 路径".into())
}
#[cfg(not(windows))]
pub fn downloads() -> Result<PathBuf,String> { Err("当前版本的身份文件选择仅支持 Windows，请手动指定已有导出目录".into()) }

#[cfg(windows)]
fn choose(owner:isize, directory:Option<&Path>, suggested:Option<&str>) -> Result<Option<PathBuf>,String> {
    use std::os::windows::ffi::OsStrExt;
    #[repr(C)]
    struct OpenFileName {
        size:u32,owner:isize,instance:isize,filter:*const u16,custom:*mut u16,max_custom:u32,filter_index:u32,
        file:*mut u16,max_file:u32,file_title:*mut u16,max_title:u32,directory:*const u16,title:*const u16,
        flags:u32,file_offset:u16,extension_offset:u16,extension:*const u16,custom_data:isize,
        hook:usize,template:*const u16,reserved:*mut std::ffi::c_void,reserved_num:u32,flags_ex:u32,
    }
    #[link(name="comdlg32")]
    unsafe extern "system" { fn GetOpenFileNameW(form:*mut OpenFileName)->i32; fn GetSaveFileNameW(form:*mut OpenFileName)->i32; fn CommDlgExtendedError()->u32; }
    let mut file = vec![0u16;32768];
    if let Some(name) = suggested { for (to,c) in file.iter_mut().zip(name.encode_utf16()) { *to=c; } }
    let filter:Vec<u16> = "ZIP 备份 (*.zip)\0*.zip\0\0".encode_utf16().collect();
    let initial:Vec<u16> = directory.map(|p|p.as_os_str().encode_wide().chain(Some(0)).collect()).unwrap_or_default();
    let title:Vec<u16> = if suggested.is_some() { "保存 FISH Dock 身份 ZIP\0" } else { "选择 FISH Dock / OFM 身份 ZIP\0" }.encode_utf16().collect();
    let extension:Vec<u16> = "zip\0".encode_utf16().collect();
    let mut form:OpenFileName = unsafe { std::mem::zeroed() };
    form.size=std::mem::size_of::<OpenFileName>() as u32; form.owner=owner; form.filter=filter.as_ptr(); form.filter_index=1;
    form.file=file.as_mut_ptr(); form.max_file=file.len() as u32; form.directory=if initial.is_empty(){std::ptr::null()}else{initial.as_ptr()};
    form.title=title.as_ptr(); form.extension=extension.as_ptr();
    // 禁止修改进程工作目录；打开时必须选已有文件。覆盖另名由保存后的 create_new 再把关。
    form.flags=0x00080000 | 0x00000008 | 0x00000800 | if suggested.is_some(){0x00000002}else{0x00001000};
    let ok=unsafe{if suggested.is_some(){GetSaveFileNameW(&mut form)}else{GetOpenFileNameW(&mut form)}};
    if ok==0 { return if unsafe{CommDlgExtendedError()}==0 {Ok(None)}else{Err("系统文件选择窗口失败，尚未保存或导入".into())}; }
    let n=file.iter().position(|c|*c==0).ok_or("系统返回的文件路径无效")?;
    let path=PathBuf::from(String::from_utf16(&file[..n]).map_err(|_|"系统返回的文件路径不是有效 Unicode")?);
    if !path.is_absolute(){return Err("系统返回的路径不是绝对路径".into());}
    crate::identity_store::safe(&path)?;
    Ok(Some(path))
}
#[cfg(windows)]
pub fn save(owner:isize,directory:&Path,name:&str)->Result<Option<PathBuf>,String>{choose(owner,Some(directory),Some(name))}
#[cfg(windows)]
pub fn open(owner:isize)->Result<Option<PathBuf>,String>{choose(owner,None,None)}
#[cfg(not(windows))]
pub fn save(_owner:isize,_directory:&Path,_name:&str)->Result<Option<PathBuf>,String>{Err("身份文件选择仅支持 Windows".into())}
#[cfg(not(windows))]
pub fn open(_owner:isize)->Result<Option<PathBuf>,String>{Err("身份文件选择仅支持 Windows".into())}
