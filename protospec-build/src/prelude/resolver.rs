use super::*;
pub struct PreludeImportResolver(pub Box<dyn ImportResolver>);

impl ImportResolver for PreludeImportResolver {
    fn clone(&self) -> Box<dyn ImportResolver>{
        Box::new(PreludeImportResolver(self.0.clone()))
    }

    fn normalize_import(&self, import: &str) -> Result<String> {
        self.0.normalize_import(import)
    }

    fn resolve_import(&self, import: &str) -> Result<Option<String>> {
        self.0.resolve_import(import)
    }

    fn resolve_ffi_transform(&self, transform: &str) -> Result<Option<ForeignTransformObj>> {
        Ok(match transform {
            "base64" => Some(Box::new(Base64Transform)),
            "gzip" => Some(Box::new(GzipTransform)),
            x => self.0.resolve_ffi_transform(x)?,
        })
    }

    fn resolve_ffi_type(&self, import: &str) -> Result<Option<ForeignTypeObj>> {
        Ok(match import {
            "v8" => Some(Box::new(VarInt::new(ScalarType::I8))),
            "v16" => Some(Box::new(VarInt::new(ScalarType::I16))),
            "v32" => Some(Box::new(VarInt::new(ScalarType::I32))),
            "v64" => Some(Box::new(VarInt::new(ScalarType::I64))),
            "v128" => Some(Box::new(VarInt::new(ScalarType::I128))),
            "utf8" => Some(Box::new(Utf8)),
            "utf16" => Some(Box::new(Utf16)),
            x => self.0.resolve_ffi_type(x)?,
        })
    }

    fn resolve_ffi_function(&self, name: &str) -> Result<Option<ForeignFunctionObj>> {
        Ok(match name {
            "len" => Some(Box::new(LenFunction)),
            "pad" => Some(Box::new(PadFunction)),
            x => self.0.resolve_ffi_function(x)?,
        })
    }
}
