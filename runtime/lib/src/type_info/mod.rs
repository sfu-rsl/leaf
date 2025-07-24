pub use common::{type_info::*, types::VariantIndex};

pub(crate) trait TypeInfoExt {
    fn as_single_variant(&self) -> Option<&VariantInfo>;
    fn expect_single_variant(&self) -> &VariantInfo;
    fn as_array(&self) -> Option<&ArrayShape>;
    fn expect_array(&self) -> &ArrayShape;
    fn child_type_ids(&self, variant: Option<VariantIndex>) -> Vec<TypeId>;
    /// Returns true if the type is a slice.
    /// Here the slice is the unsized type (`[T]`) and not the pointer to it.
    fn is_slice(&self) -> bool;
    fn new_pseudo_array_from_slice(
        slice: &Self,
        len: u64,
        item_align: Alignment,
        item_size: TypeSize,
    ) -> Self;
}

pub(crate) trait FieldsShapeInfoExt {
    fn as_array(&self) -> Option<&ArrayShape>;
    fn expect_array(&self) -> &ArrayShape;
    fn as_struct(&self) -> Option<&StructShape>;
    fn expect_struct(&self) -> &StructShape;
}

impl TypeInfoExt for TypeInfo {
    #[inline]
    fn as_single_variant(&self) -> Option<&VariantInfo> {
        match self.variants.as_slice() {
            [v] => Some(v),
            _ => None,
        }
    }

    #[inline]
    fn expect_single_variant(&self) -> &VariantInfo {
        self.as_single_variant().unwrap_or_else(|| {
            panic!(
                "Expected the type to have a single variant found {:?}",
                self
            )
        })
    }

    #[inline]
    fn as_array(&self) -> Option<&ArrayShape> {
        self.as_single_variant()
            .and_then(|variant| variant.fields.as_array())
    }

    #[inline]
    fn expect_array(&self) -> &ArrayShape {
        self.as_array().unwrap_or_else(|| {
            panic!(
                "Expected the type to have a single array variant found {:?}",
                self
            )
        })
    }

    fn child_type_ids(&self, variant: Option<VariantIndex>) -> Vec<TypeId> {
        let fields = &variant
            .map(|v| self.get_variant(v).unwrap())
            .or_else(|| self.as_single_variant())
            .unwrap()
            .fields;

        use FieldsShapeInfo::*;
        match fields {
            NoFields => vec![],
            Array(ArrayShape { item_ty, .. }) => vec![*item_ty],
            Struct(shape) => shape.fields().iter().map(|f| f.ty).collect(),
            Union(_) => panic!("The child type id of a union is not deterministic"),
        }
    }

    #[inline]
    fn is_slice(&self) -> bool {
        !self.is_sized() && self.as_array().is_some()
    }

    fn new_pseudo_array_from_slice(
        slice: &Self,
        len: u64,
        item_align: Alignment,
        item_size: TypeSize,
    ) -> Self {
        let mut variant = slice.expect_single_variant().clone();
        match variant.fields {
            FieldsShapeInfo::Array(ref mut shape) => {
                shape.len = len;
            }
            _ => panic!("Invalid slice type: {:?}", slice),
        }
        TypeInfo {
            id: slice.id,
            name: slice.name.clone(),
            variants: vec![variant],
            tag: slice.tag.clone(),
            pointee_ty: None,
            align: item_align,
            size: item_size * len,
        }
    }
}

impl FieldsShapeInfoExt for FieldsShapeInfo {
    #[inline]
    fn as_array(&self) -> Option<&ArrayShape> {
        match self {
            FieldsShapeInfo::Array(shape) => Some(shape),
            _ => None,
        }
    }

    #[inline]
    fn expect_array(&self) -> &ArrayShape {
        self.as_array()
            .unwrap_or_else(|| panic!("Expected the fields shape to be an array found {:?}", self))
    }

    #[inline]
    fn as_struct(&self) -> Option<&StructShape> {
        match self {
            FieldsShapeInfo::Struct(shape) => Some(shape),
            _ => None,
        }
    }

    #[inline]
    fn expect_struct(&self) -> &StructShape {
        self.as_struct()
            .unwrap_or_else(|| panic!("Expected the fields shape to be a struct found {:?}", self))
    }
}
