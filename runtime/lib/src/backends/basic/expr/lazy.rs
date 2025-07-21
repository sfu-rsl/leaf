use common::type_info::{FieldInfo, TypeInfo};
use common::{log_debug, log_warn};

use crate::type_info::TypeInfoExt;

use crate::backends::basic as backend;
use backend::alias::TypeDatabase;

use super::*;

mod retrieval {
    use core::num::Wrapping;

    use common::type_info::{FieldsShapeInfo, StructShape, VariantInfo};
    use common::types::TypeSize;

    use crate::abs::{IntType, ValueType, backend::CoreTypeProvider};

    use super::*;

    use backend::SymValueRefExprBuilder;

    pub(crate) trait RawPointerRetriever {
        fn retrieve(&self, addr: RawAddress, type_id: TypeId) -> ValueRef;
    }

    impl RawConcreteValue {
        pub(crate) fn type_as_scalar(
            &self,
            type_manager: &dyn TypeDatabase,
        ) -> Result<ScalarType, &LazyTypeInfo> {
            type_manager
                .try_to_value_type(self.1.clone())
                .map(Into::into)
                .or_else(|| {
                    self.1
                        .get_type(type_manager)
                        .and_then(|t| t.try_into().ok())
                })
                .ok_or(&self.1)
        }

        /// Tries to retrieve the value from a scalar type.
        /// Scalar types have no fields and no risk of nested symbolic value.
        /// Thus, they can be directly retrieved from memory.
        /// Returns the value if the type information is available.
        /// # Safety
        /// The value must be retrieved within the period that the address is still
        /// pointing to the desired memory location.
        /// This should be ensured by the caller.
        pub(crate) unsafe fn try_retrieve_as_scalar(
            &self,
            type_manager: &dyn TypeDatabase,
        ) -> Result<ConstValue, &LazyTypeInfo> {
            self.type_as_scalar(type_manager)
                .map(|ty| retrieve_scalar(self.0, &ty))
        }

        /// Retrieves the value from the raw pointer from a possibly structured type.
        /// Types that have fields or possible nested symbolic values need to be
        /// queried from the engines memory.
        /// # Safety
        /// The value and its dependents (like items of an array) must be retrieved
        /// within the period that the address is still pointing to the desired
        /// memory location.
        /// This should be ensured by the caller.
        pub(crate) unsafe fn retrieve(
            &self,
            type_manager: &dyn TypeDatabase,
            field_retriever: &dyn RawPointerRetriever,
        ) -> Result<ConcreteValueRef, &LazyTypeInfo> {
            let ty = match &self.1 {
                LazyTypeInfo::IdPrimitive(_, ty) => {
                    return Ok(ConcreteValueRef::new(
                        retrieve_scalar(self.0, &ty.clone().into()).to_value_ref(),
                    ));
                }
                LazyTypeInfo::Id(..) | LazyTypeInfo::Fetched(..) | LazyTypeInfo::Forced(..) => {
                    self.1.get_type(type_manager).unwrap()
                }
                LazyTypeInfo::None => return Err(&self.1),
            };

            Ok(self.retrieve_with_type(ty, type_manager, field_retriever))
        }

        unsafe fn retrieve_with_type(
            &self,
            ty: &TypeInfo,
            type_manager: &dyn TypeDatabase,
            field_retriever: &dyn RawPointerRetriever,
        ) -> ConcreteValueRef {
            let result = retrieve_by_type(self.0, ty, type_manager, field_retriever);

            debug_assert!(
                if let ConcreteValue::Unevaluated(UnevalValue::Lazy(_)) = &result {
                    false
                } else {
                    true
                },
                "RawConcreteValue should not be unevaluated after evaluation."
            );
            ConcreteValueRef::new(result.to_value_ref())
        }
    }

    impl LazyTypeInfo {
        pub(crate) fn fetch<'a>(&'a mut self, type_manager: &dyn TypeDatabase) -> &'a TypeInfo {
            match self {
                Self::Id(ty_id) => {
                    *self = Self::Fetched(type_manager.get_type(ty_id));
                }
                Self::IdPrimitive(ty_id, _) => {
                    *self = Self::Fetched(type_manager.get_type(ty_id));
                }
                Self::Fetched(..) | Self::Forced(..) | Self::None => {}
            }
            match self {
                Self::Fetched(ty) => *ty,
                Self::Forced(ref ty) => ty.as_ref(),
                Self::None => panic!("Type info is not available."),
                Self::Id(..) | Self::IdPrimitive(..) => unreachable!(),
            }
        }

        #[inline]
        pub(crate) fn get_type(&self, type_manager: &dyn TypeDatabase) -> Option<&TypeInfo> {
            match self {
                Self::Id(ty_id) | Self::IdPrimitive(ty_id, _) => {
                    Some(type_manager.get_type(&ty_id))
                }
                Self::Fetched(ty) => Some(*ty),
                Self::Forced(ty) => Some(ty.as_ref()),
                Self::None => None,
            }
        }

        #[inline]
        pub(crate) fn get_size(&self, type_manager: &dyn TypeDatabase) -> Option<TypeSize> {
            match self {
                Self::None => None,
                Self::IdPrimitive(_, ty) => Some(ty.size().into()),
                _ => match self {
                    Self::Id(ty_id) => type_manager.get_type(&ty_id),
                    Self::Fetched(ty) => *ty,
                    Self::Forced(ty) => ty.as_ref(),
                    _ => unreachable!(),
                }
                .size(),
            }
        }
    }

    #[derive(Debug, Clone, Copy, derive_more::From)]
    pub(crate) enum ScalarType {
        Bool,
        Char,
        Int(IntType),
        Float(FloatType),
        Address,
    }

    impl ScalarType {
        fn try_from<'a>(
            ty: &'a TypeInfo,
            core_types: &(impl CoreTypeProvider<&'a TypeInfo> + ?Sized),
        ) -> Result<Self, ()> {
            if ty.id == core_types.raw_addr().id || ty.id == core_types.raw_mut_addr().id {
                Ok(Self::Address)
            } else if let Some(value_ty) = core_types.try_to_value_type(ty) {
                Ok(Self::from(value_ty))
            } else if ty.pointee_ty.is_some() && ty.size() == core_types.raw_addr().size() {
                Ok(Self::Address)
            } else {
                Err(())
            }
        }
    }

    impl<'a> TryFrom<&'a TypeInfo> for ScalarType {
        type Error = ();

        fn try_from(ty: &'a TypeInfo) -> Result<Self, Self::Error> {
            // TODO: To be replaced with a well-cached implementation
            let name = ty.name.as_str();
            match name {
                "bool" => Ok(ScalarType::Bool),
                "char" => Ok(ScalarType::Char),
                _ if name.starts_with('i') || name.starts_with('u') => name[1..]
                    .parse()
                    .map(|bit_size| {
                        IntType {
                            bit_size,
                            is_signed: name.starts_with('i'),
                        }
                        .into()
                    })
                    .or_else(|_| {
                        if name[1..] == *"size" {
                            Ok(IntType {
                                is_signed: name.starts_with('i'),
                                ..IntType::USIZE
                            }
                            .into())
                        } else {
                            Err(())
                        }
                    }),
                "f32" | "f64" => unimplemented!(),
                _ if ty.pointee_ty.is_some() => match ty.as_single_variant() {
                    Some(VariantInfo {
                        fields: FieldsShapeInfo::NoFields,
                        ..
                    }) => Ok(ScalarType::Address),
                    _ => Err(()),
                },
                _ => Err(()),
            }
        }
    }

    impl From<ValueType> for ScalarType {
        fn from(ty: ValueType) -> Self {
            match ty {
                ValueType::Bool => ScalarType::Bool,
                ValueType::Char => ScalarType::Char,
                ValueType::Int(ty) => ScalarType::Int(ty),
                ValueType::Float(ty) => ScalarType::Float(ty),
            }
        }
    }

    impl From<ScalarType> for ValueType {
        fn from(value: ScalarType) -> Self {
            match value {
                ScalarType::Bool => ValueType::Bool,
                ScalarType::Char => ValueType::Char,
                ScalarType::Int(ty) => ValueType::Int(ty),
                ScalarType::Float(ty) => ValueType::Float(ty),
                ScalarType::Address => {
                    log_warn!("Converting address type to int type.");
                    ValueType::Int(IntType {
                        bit_size: std::mem::size_of::<usize>() as u64 * 8,
                        is_signed: false,
                    })
                }
            }
        }
    }

    impl ScalarType {
        fn bit_size(&self) -> u64 {
            match self {
                ScalarType::Bool => core::mem::size_of::<bool>() as u64 * 8,
                ScalarType::Char => core::mem::size_of::<char>() as u64 * 8,
                ScalarType::Int(ty) => ty.bit_size,
                ScalarType::Float(ty) => ty.e_bits + ty.s_bits,
                ScalarType::Address => core::mem::size_of::<*const ()>() as u64 * 8,
            }
        }
    }

    unsafe fn retrieve_scalar(addr: RawAddress, ty: &ScalarType) -> ConstValue {
        use std::ptr::with_exposed_provenance as to_ptr;
        let addr = addr as usize;
        match ty {
            ScalarType::Bool => (*(to_ptr::<bool>(addr))).into(),
            ScalarType::Char => (*(to_ptr::<char>(addr))).into(),
            ScalarType::Int(ty @ IntType { bit_size, .. }) => ConstValue::Int {
                bit_rep: Wrapping(retrieve_int(addr, *bit_size as usize)),
                ty: *ty,
            },
            ScalarType::Float(_) => unimplemented!(),
            ScalarType::Address => ConstValue::Addr(*(to_ptr::<*const ()>(addr))),
        }
    }

    unsafe fn retrieve_int(addr: usize, bit_size: usize) -> u128 {
        let bytes =
            std::slice::from_raw_parts(std::ptr::with_exposed_provenance::<u8>(addr), bit_size / 8);

        #[cfg(target_endian = "big")]
        let bytes = bytes.iter();
        #[cfg(target_endian = "little")]
        let bytes = bytes.iter().rev();

        let mut result: u128 = 0;
        for byte in bytes {
            result = result << 8 | *byte as u128;
        }
        result
    }

    unsafe fn retrieve_by_type(
        addr: RawAddress,
        ty: &TypeInfo,
        type_manager: &dyn TypeDatabase,
        field_retriever: &dyn RawPointerRetriever,
    ) -> ConcreteValue {
        log_debug!("Retrieving value at {:p} of type: {:?}", addr, ty.id);
        let unsupported = || {
            unimplemented!(
                "Evaluation of raw concrete value with this type is not supported yet. {:?}",
                ty
            )
        };
        debug_assert!(
            ty.is_sized(),
            "Unsized types are not expected to be retrieved.",
        );
        if let Some(variant) = ty.as_single_variant() {
            use common::type_info::FieldsShapeInfo::*;
            match &variant.fields {
                Array(shape) => retrieve_array(
                    addr,
                    shape.len as usize,
                    type_manager.get_type(&shape.item_ty),
                    |addr| field_retriever.retrieve(addr, shape.item_ty),
                )
                .into(),
                NoFields => {
                    if let Ok(ty) = ScalarType::try_from(ty, type_manager) {
                        retrieve_scalar(addr, &ty).into()
                    } else {
                        unsupported()
                    }
                }
                Union(..) => unsupported(),
                Struct(shape) => match ty.pointee_ty {
                    None => retrieve_struct(addr, &shape, field_retriever).into(),
                    Some(pointee_ty) => {
                        debug_assert!(
                            !type_manager.get_type(&pointee_ty).is_sized(),
                            "Pointer with struct shape is expected to be fat."
                        );
                        retrieve_fat_ptr(addr, ty.id, &shape, field_retriever)
                    }
                },
            }
        } else {
            unsupported()
        }
    }

    fn retrieve_array(
        addr: RawAddress,
        len: usize,
        item_ty: &TypeInfo,
        retrieve_addr: impl Fn(RawAddress) -> ValueRef,
    ) -> ArrayValue {
        /* FIXME: We can do better by querying the memory in a coarser way.
         * The current memory supports checking ranges and porter values.
         * Thus we can check for the whole array region and place those symbolic
         * values at the right indices. */
        let mut elements = Vec::with_capacity(len);
        for i in 0..(len as u64) {
            let item_addr = addr.wrapping_byte_offset((i * item_ty.size) as isize);
            elements.push(retrieve_addr(item_addr));
        }
        ArrayValue { elements }
    }

    fn retrieve_struct(
        addr: RawAddress,
        shape: &StructShape,
        field_retriever: &dyn RawPointerRetriever,
    ) -> AdtValue {
        /* FIXME: We can do better by querying the memory in a coarser way.
         * The current memory supports checking ranges and porter values.
         * Thus we can check for the whole struct region and place those symbolic
         * values at the right indices. */
        AdtValue {
            kind: super::AdtKind::Struct,
            fields: shape
                .fields()
                .iter()
                .map(|f| {
                    field_retriever.retrieve(addr.wrapping_byte_offset(f.offset as isize), f.ty)
                })
                .map(Some)
                .map(Into::into)
                .collect(),
        }
    }

    fn retrieve_fat_ptr(
        addr: RawAddress,
        type_id: TypeId,
        shape: &StructShape,
        field_retriever: &dyn RawPointerRetriever,
    ) -> ConcreteValue {
        let mut adt = retrieve_struct(addr, shape, field_retriever);
        // FIXME: There is no guarantee for this structure.
        let metadata = adt.fields.pop().unwrap().value.unwrap(); // Field 1
        let address = adt.fields.pop().unwrap().value.unwrap(); // Field 0
        FatPtrValue {
            address: ConcreteValueRef::new(address),
            metadata: ConcreteValueRef::new(metadata),
            ty: type_id,
        }
        .into()
    }

    struct MaskedValueBuilder<'a, 'b, EB: SymValueRefExprBuilder> {
        type_manager: &'a dyn TypeDatabase,
        expr_builder: &'b mut EB,
    }

    impl<EB: SymValueRefExprBuilder> MaskedValueBuilder<'_, '_, EB> {
        #[tracing::instrument(level = "debug", skip(self))]
        pub(crate) fn build(&mut self, value: &PorterValue, whole_ty: ScalarType) -> SymValueRef {
            let whole_bit_rep_ty = IntType {
                bit_size: whole_ty.bit_size(),
                is_signed: false,
            };
            let whole_size = whole_ty.bit_size() / u8::BITS as TypeSize;

            let whole_bit_rep = unsafe { retrieve_scalar(value.as_concrete.0, &whole_ty) }
                .try_to_bit_rep()
                .expect("Unexpected const value.");

            let result = match value.sym_values.iter().try_fold(
                (
                    ConstValue::new_int(0u128, whole_bit_rep_ty).to_value_ref(),
                    0u128,
                ),
                |(acc_value, acc_mask), (at, ty_id, sym_value)| {
                    debug_assert!(!matches!(
                        sym_value.as_ref(),
                        SymValue::Expression(Expr::Partial(_))
                    ));

                    let value_ty = self.type_manager.get_type(ty_id);
                    let value_size = value_ty.size;

                    let sym_value = self.to_bit_rep(sym_value, value_ty);

                    // Covering the whole value
                    if value_size == whole_size {
                        debug_assert_eq!(value.sym_values.len(), 1);
                        log_debug!(
                            concat!(
                                "A transmutation is happening as a form of porter value. ",
                                "This is only expected to appear when there are ",
                                "symbolic enum discriminants and raw symbolic pointer casts. ",
                                "Whole value: {:?}",
                            ),
                            value,
                        );
                        return Err(sym_value);
                    }

                    let (padded, mask) = self.pad(sym_value, *at, value_size, whole_bit_rep_ty);

                    Ok((
                        self.expr_builder.or((padded, acc_value).into()),
                        acc_mask | mask,
                    ))
                },
            ) {
                Ok((result, mask)) => {
                    let result = SymValueRef::new(result);
                    let result = SymValueRef::new(
                        self.expr_builder.and(
                            (
                                result,
                                ConstValue::new_int(mask, whole_bit_rep_ty).to_value_ref(),
                            )
                                .into(),
                        ),
                    );
                    let result = self.expr_builder.or((
                        result,
                        ConstValue::new_int(whole_bit_rep & !mask, whole_bit_rep_ty).to_value_ref(),
                    )
                        .into());
                    SymValueRef::new(result)
                }
                Err(transmuted) => transmuted,
            };

            let result = if let ScalarType::Bool = whole_ty {
                Self::bit_rep_to_bool(result)
            } else {
                let ty_info = value.as_concrete.1.clone();
                SymValueRef::new(
                    self.expr_builder
                        .transmute(result.into(), ty_info.id().unwrap(), ty_info)
                        .into(),
                )
            };

            result
        }

        /// Creates a zero-padded value from a symbolic value + a mask representing the set bits.
        /// # Example
        /// If a symbolic u8 value at byte offset 1 in little-endian order is requested to be masked
        /// for a u32 value, the padded value will be 0x0000xx00 and the mask will be 0x0000ff00.
        fn pad(
            &mut self,
            value: SymValueRef,
            at: PointerOffset,
            value_size: TypeSize,
            bit_rep_ty: IntType,
        ) -> (SymValueRef, u128) {
            let whole_size = bit_rep_ty.bit_size / u8::BITS as u64;

            debug_assert!(
                value_size + at <= whole_size,
                "The value is overflowing the whole value. {:?} @ {:?} of {:?}",
                value,
                at,
                whole_size,
            );

            let padded = (0..value_size).fold(
                ConstValue::new_int(0u128, bit_rep_ty).to_value_ref(),
                |acc, i| {
                    let byte = self.extract_byte(value.clone(), value_size, i);
                    let extended = SymValueRef::new(self.expr_builder.to_int(
                        byte,
                        bit_rep_ty,
                        self.type_manager.int_type(bit_rep_ty),
                    ));
                    // NOTE: We need to care about the endianness as the whole value is a scalar (primitive).
                    let shift_amount = Self::shift_amount(whole_size, at + i as PointerOffset);
                    let aligned = SymValueRef::new(
                        self.expr_builder
                            .shl(SymBinaryOperands::Orig {
                                first: extended,
                                second: ConstValue::new_int(shift_amount, bit_rep_ty)
                                    .to_value_ref(),
                            })
                            .into(),
                    );
                    self.expr_builder.or((aligned, acc).into())
                },
            );

            let mask = IntType::bit_mask(value_size as u32 * u8::BITS)
                << Self::shift_amount(whole_size, at);

            (SymValueRef::new(padded), mask)
        }

        /// # Parameters
        /// - `byte_offset`: The offset of the byte to be extracted from location where the value
        /// would be located.
        /// # Remarks
        /// The return value that has the type of u8.
        fn extract_byte(
            &mut self,
            value: SymValueRef,
            value_size: TypeSize,
            byte_offset: PointerOffset,
        ) -> SymValueRef {
            // Shift right and truncate.
            let result = value;

            let shift_amount = Self::shift_amount(value_size, byte_offset);
            let result = SymValueRef::new(
                self.expr_builder
                    .shr(SymBinaryOperands::Orig {
                        first: result,
                        second: ConstValue::from(shift_amount).to_value_ref(),
                    })
                    .into(),
            );

            let result = SymValueRef::new(self.expr_builder.to_int(
                result,
                IntType::U8,
                self.type_manager.u8(),
            ));

            debug_assert!(
                ValueType::try_from(result.value()).is_ok_and(|ty| ty == IntType::U8.into()),
            );
            result
        }

        fn to_bit_rep(&mut self, value: &SymValueRef, ty: &TypeInfo) -> SymValueRef {
            if ScalarType::try_from(ty, self.type_manager)
                .is_ok_and(|ty| matches!(ty, ScalarType::Bool))
            {
                SymValueRef::new(self.expr_builder.to_int(
                    value.clone(),
                    IntType::U8,
                    self.type_manager.u8(),
                ))
            } else {
                // There is nothing special to do for other types.
                value.clone()
            }
        }

        #[cfg_attr(target_endian = "little", allow(unused))]
        fn shift_amount(size: TypeSize, offset: PointerOffset) -> u32 {
            #[cfg(target_endian = "big")]
            let shift_amount_byte = size - offset - 1;
            #[cfg(target_endian = "little")]
            let shift_amount_byte = offset;
            shift_amount_byte as u32 * u8::BITS
        }

        fn bit_rep_to_bool(result: SymValueRef) -> SymValueRef {
            BinaryExpr {
                operator: BinaryOp::Ne,
                operands: (result, ConstValue::from(0_u8).to_value_ref()).into(),
            }
            .to_value_ref()
        }
    }

    impl PorterValue {
        pub(crate) fn try_to_masked_value<'a, 'b, EB: SymValueRefExprBuilder>(
            &self,
            type_manager: &'a dyn TypeDatabase,
            expr_builder: &'b mut EB,
        ) -> Result<SymValueRef, &LazyTypeInfo> {
            let whole_ty = self.as_concrete.type_as_scalar(type_manager)?;
            let mut builder = MaskedValueBuilder {
                type_manager,
                expr_builder,
            };
            Ok(builder.build(self, whole_ty))
        }
    }

    impl ConcreteValue {
        pub(crate) fn try_resolve_as_const(
            &self,
            type_manager: &dyn TypeDatabase,
        ) -> Option<ConstValue> {
            match self {
                ConcreteValue::Const(value) => Some(value.clone()),
                ConcreteValue::Unevaluated(UnevalValue::Lazy(lazy)) => {
                    unsafe { lazy.try_retrieve_as_scalar(type_manager) }.ok()
                }
                _ => None,
            }
        }
    }
}

pub(crate) use retrieval::RawPointerRetriever;

mod proj {
    use super::*;

    fn pseudo_array_ty_from_slice(
        slice_ty: &TypeInfo,
        len: u64,
        type_manager: &dyn TypeDatabase,
    ) -> TypeInfo {
        debug_assert!(slice_ty.is_slice());
        let item_ty = type_manager.get_type(&slice_ty.expect_array().item_ty);
        let array_ty =
            TypeInfo::new_pseudo_array_from_slice(&slice_ty, len, item_ty.align, item_ty.size);
        log_debug!("Pseudo array type: {:?}", array_ty);
        array_ty
    }

    impl FatPtrValue {
        pub(crate) fn deref(
            &self,
            type_manager: &dyn TypeDatabase,
            retriever: &dyn RawPointerRetriever,
        ) -> RawConcreteValue {
            let addr = self.address.expect_addr(type_manager, retriever);
            let ty = type_manager.get_type(&self.ty);
            let pointee_ty = type_manager.get_type(&ty.pointee_ty.unwrap());
            let value = if pointee_ty.is_slice() {
                // Simulate an array.
                let len = self
                    .metadata
                    .expect_int(type_manager, retriever)
                    .try_into()
                    .unwrap();
                let array_ty = pseudo_array_ty_from_slice(pointee_ty, len, type_manager);
                RawConcreteValue(addr, LazyTypeInfo::Forced(Rc::new(array_ty)))
            } else {
                // Do we need to worry about the loss of metadata? Is it going to be processed further?
                RawConcreteValue(addr, LazyTypeInfo::Id(pointee_ty.id))
            };
            value
        }
    }

    impl ConcreteValue {
        pub(crate) fn expect_int(
            &self,
            type_manager: &dyn TypeDatabase,
            retriever: &dyn RawPointerRetriever,
        ) -> u128 {
            match self {
                ConcreteValue::Const(ConstValue::Int { bit_rep, .. }) => bit_rep.0,
                ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
                    unsafe { raw.retrieve(type_manager, retriever) }
                        .unwrap()
                        .expect_int(type_manager, retriever)
                }
                _ => panic!("Not an integer: {:?}", self),
            }
        }

        pub(crate) fn expect_addr(
            &self,
            type_manager: &dyn TypeDatabase,
            retriever: &dyn RawPointerRetriever,
        ) -> RawAddress {
            match self {
                ConcreteValue::Const(ConstValue::Addr(addr)) => *addr,
                ConcreteValue::Unevaluated(UnevalValue::Lazy(raw)) => {
                    unsafe { raw.retrieve(type_manager, retriever) }
                        .unwrap()
                        .expect_addr(type_manager, retriever)
                }
                _ => panic!("Not an address: {:?}", self),
            }
        }
    }
}
