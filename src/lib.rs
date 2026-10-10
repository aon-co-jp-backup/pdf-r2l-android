//! PDFの綴じ方向(R2L/L2R)を変更するコア。AndroidからはJNI経由(`Native.convert`)で呼ぶ。
use jni::objects::{JByteArray, JObject};
use jni::sys::jint;
use jni::JNIEnv;
use lopdf::{Dictionary, Document, Object};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    R2L,
    L2R,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::R2L => "R2L",
            Mode::L2R => "L2R",
        }
    }
}

/// ViewerPreferences の Direction を設定した PDF のバイト列を返す。
pub fn convert_bytes(src: &[u8], mode: Mode) -> Result<Vec<u8>, String> {
    let mut doc = Document::load_mem(src).map_err(|e| format!("読み込み失敗: {e}"))?;
    let root_id = doc
        .trailer
        .get(b"Root")
        .and_then(|o| o.as_reference())
        .map_err(|e| format!("Catalogが見つかりません: {e}"))?;

    let vp_ref = doc
        .get_dictionary(root_id)
        .ok()
        .and_then(|c| c.get(b"ViewerPreferences").ok())
        .and_then(|o| o.as_reference().ok());
    let dir = Object::Name(mode.name().as_bytes().to_vec());
    if let Some(id) = vp_ref {
        doc.get_dictionary_mut(id)
            .map_err(|e| e.to_string())?
            .set("Direction", dir);
    } else {
        let catalog = doc.get_dictionary_mut(root_id).map_err(|e| e.to_string())?;
        match catalog.get_mut(b"ViewerPreferences") {
            Ok(Object::Dictionary(d)) => d.set("Direction", dir),
            _ => {
                let mut d = Dictionary::new();
                d.set("Direction", dir);
                catalog.set("ViewerPreferences", Object::Dictionary(d));
            }
        }
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| format!("保存失敗: {e}"))?;
    Ok(out)
}

/// Kotlin: `external fun convert(input: ByteArray, mode: Int): ByteArray`(mode 0=R2L, 1=L2R)。
/// 失敗時は RuntimeException を投げる。
#[no_mangle]
pub extern "system" fn Java_tokyo_aon_pdfr2l_Native_convert<'l>(
    mut env: JNIEnv<'l>,
    _this: JObject<'l>,
    input: JByteArray<'l>,
    mode: jint,
) -> JByteArray<'l> {
    let null = || unsafe { JByteArray::from_raw(std::ptr::null_mut()) };
    let bytes = match env.convert_byte_array(&input) {
        Ok(b) => b,
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", e.to_string());
            return null();
        }
    };
    let m = if mode == 1 { Mode::L2R } else { Mode::R2L };
    match convert_bytes(&bytes, m) {
        Ok(out) => env.byte_array_from_slice(&out).unwrap_or_else(|_| null()),
        Err(msg) => {
            let _ = env.throw_new("java/lang/RuntimeException", msg);
            null()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn sample() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()]});
        doc.objects.insert(pages, Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}));
        let cat = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        doc.trailer.set("Root", cat);
        let mut v = Vec::new();
        doc.save_to(&mut v).unwrap();
        v
    }

    fn dir_of(bytes: &[u8]) -> Vec<u8> {
        let doc = Document::load_mem(bytes).unwrap();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let vp = doc.get_dictionary(root).unwrap().get(b"ViewerPreferences").unwrap().as_dict().unwrap();
        vp.get(b"Direction").unwrap().as_name().unwrap().to_vec()
    }

    #[test]
    fn sets_direction() {
        let b = convert_bytes(&sample(), Mode::R2L).unwrap();
        assert_eq!(dir_of(&b), b"R2L");
        let c = convert_bytes(&b, Mode::L2R).unwrap();
        assert_eq!(dir_of(&c), b"L2R");
        assert!(convert_bytes(b"not a pdf", Mode::R2L).is_err());
    }
}
