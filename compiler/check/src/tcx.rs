mod expr;
mod item;
mod ty;
mod unify;
mod verify;

use crate::constraint::*;
use crate::layout::Layout;
use crate::layout::*;
use crate::sharded::ShardedHashMap;
use crate::ty::*;
use diagnostics::{Reporter, Span};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

type InternedSet<'tcx, T> = ShardedHashMap<Interned<'tcx, T>, ()>;

pub struct Tcx<'tcx> {
    pub reporter: &'tcx Reporter,
    pub(crate) intern: TcxIntern<'tcx>,
    pub package: &'tcx hir::Package,
    pub module_structure: &'tcx hir::resolve::ModuleStructure,
    pub(crate) target: &'tcx target_lexicon::Triple,
    types: RefCell<BTreeMap<hir::Id, Ty<'tcx>>>,
    layouts: RefCell<HashMap<*const Type<'tcx>, TyLayout<'tcx, Ty<'tcx>>>>,
    constraints: RefCell<Constraints<'tcx>>,
    pub(crate) substs: RefCell<HashMap<*const Type<'tcx>, HashMap<hir::Id, Ty<'tcx>>>>,
    ty_vars: Cell<usize>,
    pub builtin: BuiltinTypes<'tcx>,
    pub lang_items: hir::lang::LangItems,
}

pub(crate) struct TcxIntern<'tcx> {
    arena: &'tcx bumpalo::Bump,
    types: InternedSet<'tcx, Type<'tcx>>,
    type_list: InternedSet<'tcx, List<Ty<'tcx>>>,
    field_list: InternedSet<'tcx, List<Field<'tcx>>>,
    variant_list: InternedSet<'tcx, List<Variant<'tcx>>>,
    param_list: InternedSet<'tcx, List<Param<'tcx>>>,
    id_list: InternedSet<'tcx, List<hir::Id>>,
    layout: InternedSet<'tcx, Layout>,
}

pub struct BuiltinTypes<'tcx> {
    pub error: Ty<'tcx>,
    pub never: Ty<'tcx>,
    pub unit: Ty<'tcx>,
    pub bool: Ty<'tcx>,
    pub str: Ty<'tcx>,
    pub typeid: Ty<'tcx>,
    pub u8: Ty<'tcx>,
    pub u16: Ty<'tcx>,
    pub u32: Ty<'tcx>,
    pub u64: Ty<'tcx>,
    pub u128: Ty<'tcx>,
    pub usize: Ty<'tcx>,
    pub i8: Ty<'tcx>,
    pub i16: Ty<'tcx>,
    pub i32: Ty<'tcx>,
    pub i64: Ty<'tcx>,
    pub i128: Ty<'tcx>,
    pub isize: Ty<'tcx>,
    pub f32: Ty<'tcx>,
    pub f64: Ty<'tcx>,
    pub ref_unit: Ty<'tcx>,
    pub ref_u8: Ty<'tcx>,
    pub type_layout: Ty<'tcx>,
}

impl<'tcx> BuiltinTypes<'tcx> {
    fn new(intern: &TcxIntern<'tcx>) -> Self {
        let unit = intern.intern_ty(Type::Tuple(List::empty()));
        let u8 = intern.intern_ty(Type::UInt(8));
        let usize = intern.intern_ty(Type::UInt(0));

        BuiltinTypes {
            error: intern.intern_ty(Type::Error),
            never: intern.intern_ty(Type::Never),
            unit,
            bool: intern.intern_ty(Type::Bool),
            str: intern.intern_ty(Type::Str),
            typeid: intern.intern_ty(Type::TypeId),
            u8,
            u16: intern.intern_ty(Type::UInt(16)),
            u32: intern.intern_ty(Type::UInt(32)),
            u64: intern.intern_ty(Type::UInt(64)),
            u128: intern.intern_ty(Type::UInt(128)),
            usize,
            i8: intern.intern_ty(Type::Int(8)),
            i16: intern.intern_ty(Type::Int(16)),
            i32: intern.intern_ty(Type::Int(32)),
            i64: intern.intern_ty(Type::Int(64)),
            i128: intern.intern_ty(Type::Int(128)),
            isize: intern.intern_ty(Type::Int(0)),
            f32: intern.intern_ty(Type::Float(32)),
            f64: intern.intern_ty(Type::Float(64)),
            ref_unit: intern.intern_ty(Type::Ref(false, unit)),
            ref_u8: intern.intern_ty(Type::Ref(false, u8)),
            type_layout: intern.intern_ty(Type::Tuple(
                intern.intern_ty_list(&[&*usize, &*usize, &*usize]),
            )),
        }
    }
}

impl<'tcx> TcxIntern<'tcx> {
    pub(crate) fn intern_ty(&self, ty: Type<'tcx>) -> Ty<'tcx> {
        self.types.intern(ty, |ty| Interned(self.arena.alloc(ty))).0
    }

    pub(crate) fn intern_ty_list(&self, list: &[Ty<'tcx>]) -> &'tcx List<Ty<'tcx>> {
        if list.is_empty() {
            List::empty()
        } else {
            self.type_list
                .intern_ref(list, || Interned(List::from_arena(self.arena, list)))
                .0
        }
    }

    pub(crate) fn intern_field_list(&self, list: &[Field<'tcx>]) -> &'tcx List<Field<'tcx>> {
        if list.is_empty() {
            List::empty()
        } else {
            self.field_list
                .intern_ref(list, || Interned(List::from_arena(self.arena, list)))
                .0
        }
    }

    pub(crate) fn intern_variant_list(&self, list: &[Variant<'tcx>]) -> &'tcx List<Variant<'tcx>> {
        if list.is_empty() {
            List::empty()
        } else {
            self.variant_list
                .intern_ref(list, || Interned(List::from_arena(self.arena, list)))
                .0
        }
    }

    pub(crate) fn intern_param_list(&self, list: &[Param<'tcx>]) -> &'tcx List<Param<'tcx>> {
        if list.is_empty() {
            List::empty()
        } else {
            self.param_list
                .intern_ref(list, || Interned(List::from_arena(self.arena, list)))
                .0
        }
    }

    pub(crate) fn intern_id_list(&self, list: &[hir::Id]) -> &'tcx List<hir::Id> {
        if list.is_empty() {
            List::empty()
        } else {
            self.id_list
                .intern_ref(list, || Interned(List::from_arena(self.arena, list)))
                .0
        }
    }

    pub(crate) fn intern_layout(&self, layout: Layout) -> &'tcx Layout {
        self.layout
            .intern(layout, |layout| Interned(self.arena.alloc(layout)))
            .0
    }
}

impl<'tcx> Tcx<'tcx> {
    pub fn new(
        reporter: &'tcx Reporter,
        arena: &'tcx bumpalo::Bump,
        target: &'tcx target_lexicon::Triple,
        package: &'tcx hir::Package,
        module_structure: &'tcx hir::resolve::ModuleStructure,
    ) -> Self {
        let intern = TcxIntern {
            arena,
            types: InternedSet::default(),
            type_list: InternedSet::default(),
            field_list: InternedSet::default(),
            variant_list: InternedSet::default(),
            param_list: InternedSet::default(),
            id_list: InternedSet::default(),
            layout: InternedSet::default(),
        };

        Tcx {
            builtin: BuiltinTypes::new(&intern),
            reporter,
            intern,
            target,
            package,
            module_structure,
            lang_items: hir::lang::LangItems::collect(package),
            types: RefCell::new(BTreeMap::new()),
            layouts: RefCell::new(HashMap::new()),
            constraints: RefCell::new(Constraints::new()),
            substs: RefCell::new(HashMap::new()),
            ty_vars: Cell::new(0),
        }
    }

    pub fn type_of(&self, id: &hir::Id) -> Ty<'tcx> {
        let types = self.types.borrow();

        if let Some(ty) = types.get(id) {
            ty
        } else {
            std::mem::drop(types);

            let ty = if let Some(_) = self.package.exprs.get(id) {
                self.infer_expr(id)
            } else if let Some(_) = self.package.types.get(id) {
                self.infer_type(id)
            } else if let Some(_) = self.package.items.get(id) {
                let ty = self.infer_item(id);

                self.types.borrow_mut().insert(*id, ty);
                self.check_item(id);
                self.types.borrow()[id]
            } else {
                panic!("unused id {}", id);
            };

            self.types.borrow_mut().insert(*id, ty);

            ty
        }
    }

    pub fn span_of(&self, id: &hir::Id) -> Span {
        if let Some(expr) = self.package.exprs.get(id) {
            expr.span
        } else if let Some(ty) = self.package.types.get(id) {
            ty.span
        } else if let Some(item) = self.package.items.get(id) {
            item.span
        } else {
            panic!("unused id {}", id);
        }
    }

    pub fn subst_of(&self, ty: Ty<'tcx>) -> Option<std::cell::Ref<HashMap<hir::Id, Ty<'tcx>>>> {
        let substs = self.substs.borrow();

        if substs.contains_key(&(ty as *const _)) {
            let ty = ty as *const Type<'tcx> as usize;

            Some(std::cell::Ref::map(
                self.substs.borrow(),
                move |substs: &HashMap<*const Type<'tcx>, HashMap<hir::Id, Ty<'tcx>>>| {
                    let ty = ty as *const Type;
                    substs.get(&ty).unwrap()
                },
            ))
        } else {
            None
        }
    }

    pub fn constrain(&self, cs: Constraint<'tcx>) {
        self.constraints.borrow_mut().push(cs);
    }

    pub fn new_var(&self) -> Ty<'tcx> {
        let var = self.ty_vars.get();

        self.ty_vars.set(var + 1);
        self.intern_ty(Type::Var(TypeVar(var)))
    }

    pub fn new_int(&self) -> Ty<'tcx> {
        let var = self.ty_vars.get();

        self.ty_vars.set(var + 1);
        self.intern_ty(Type::VInt(TypeVar(var)))
    }

    pub fn new_uint(&self) -> Ty<'tcx> {
        let var = self.ty_vars.get();

        self.ty_vars.set(var + 1);
        self.intern_ty(Type::VUInt(TypeVar(var)))
    }

    pub fn new_float(&self) -> Ty<'tcx> {
        let var = self.ty_vars.get();

        self.ty_vars.set(var + 1);
        self.intern_ty(Type::VFloat(TypeVar(var)))
    }

    pub fn get_full_name(&self, id: &hir::Id, base: bool) -> String {
        let mut path = Vec::new();

        if self.module_structure.find_path(id, &mut path) {
            if base {
                path.into_iter()
                    .chain(std::iter::once(self.module_structure.name))
                    .rev()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                path.into_iter()
                    .rev()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join("/")
            }
        } else {
            self.package.items[id].name.to_string()
        }
    }

    pub fn store_type_map(&self, file: impl AsRef<std::path::Path>) {
        let mut tmap = TypeMap(HashMap::new());

        fn rec<'tcx>(
            this: &Tcx<'tcx>,
            tmap: &mut TypeMap<'tcx>,
            m: &hir::resolve::ModuleStructure,
        ) {
            for (_, (id, _)) in &m.items {
                tmap.0.insert(*id, this.type_of(id));
            }

            for child in &m.children {
                rec(this, tmap, child);
            }
        }

        rec(self, &mut tmap, self.module_structure);

        let file = file.as_ref();
        let file = std::fs::File::create(file).unwrap();

        bincode::serialize_into(file, &tmap).unwrap();
    }

    pub fn load_type_map(&self, file: impl AsRef<std::path::Path>) {
        use bincode::Options;
        let file = file.as_ref();
        let file = std::fs::File::open(file).unwrap();
        let tmap: TypeMap = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .deserialize_from_seed(crate::ty::ser::Deser(&self.intern), file)
            .unwrap();

        let mut types = self.types.borrow_mut();

        for (id, ty) in tmap.0 {
            types.insert(id, ty);
        }
    }

    pub fn bug(&self, code: impl Into<Option<u16>>, msg: impl Into<String>, span: Span) {
        self.reporter.add(
            diagnostics::Diagnostic::new(diagnostics::Severity::Bug, code, msg).label(
                diagnostics::Severity::Bug,
                span,
                None::<String>,
            ),
        );
    }

    pub fn error(&self, code: impl Into<Option<u16>>, msg: impl Into<String>, span: Span) {
        self.reporter.add(
            diagnostics::Diagnostic::new(diagnostics::Severity::Warning, code, msg).label(
                diagnostics::Severity::Warning,
                span,
                None::<String>,
            ),
        );
    }

    pub fn warm(&self, code: impl Into<Option<u16>>, msg: impl Into<String>, span: Span) {
        self.reporter.add(
            diagnostics::Diagnostic::new(diagnostics::Severity::Warning, code, msg).label(
                diagnostics::Severity::Warning,
                span,
                None::<String>,
            ),
        );
    }

    pub fn layout_of(&self, id: &hir::Id) -> TyLayout<'tcx, Ty<'tcx>> {
        self.layout(self.type_of(id))
    }

    pub fn layout(&self, ty: Ty<'tcx>) -> TyLayout<'tcx, Ty<'tcx>> {
        let layouts = self.layouts.borrow();

        if let Some(layout) = layouts.get(&(ty as *const _)) {
            return *layout;
        }

        std::mem::drop(layouts);

        let scalar_unit = |value: Primitive| {
            let bits = value.size(self.target).bits();
            assert!(bits <= 128);
            Scalar {
                value,
                valid_range: 0..=(!0 >> (128 - bits)),
            }
        };

        let scalar =
            |value: Primitive| self.intern_layout(Layout::scalar(scalar_unit(value), self.target));

        let layout = match ty {
            Type::Error => unreachable!(),
            Type::Var(_) => unreachable!(),
            Type::Forall(_, of) => return self.layout(of),
            Type::TypeOf(id, args) => {
                return self.layout(self.type_of(id).mono(self, args.to_vec()))
            }
            Type::Param(_) => {
                let mut data = scalar_unit(Primitive::Pointer);
                let mut meta = scalar_unit(Primitive::Pointer);

                data.valid_range = 1..=*data.valid_range.start();
                meta.valid_range = 1..=*data.valid_range.start();

                self.intern_layout(self.scalar_pair(data, meta))
            }
            Type::VInt(_) => match self.target.pointer_width() {
                Ok(target_lexicon::PointerWidth::U16) => scalar(Primitive::Int(Integer::I16, true)),
                Ok(target_lexicon::PointerWidth::U32) => scalar(Primitive::Int(Integer::I32, true)),
                Ok(target_lexicon::PointerWidth::U64) => scalar(Primitive::Int(Integer::I64, true)),
                Err(_) => scalar(Primitive::Int(Integer::I32, true)),
            },
            Type::VUInt(_) => unreachable!(),
            Type::VFloat(_) => unreachable!(),
            Type::Never => self.intern_layout(Layout {
                fields: FieldsShape::Primitive,
                variants: Variants::Single { index: 0 },
                largest_niche: None,
                abi: Abi::Uninhabited,
                size: Size::ZERO,
                align: Align::from_bits(8),
                stride: Size::ZERO,
            }),
            Type::Bool => self.intern_layout(Layout::scalar(
                Scalar {
                    value: Primitive::Int(Integer::I8, false),
                    valid_range: 0..=1,
                },
                self.target,
            )),
            Type::Int(0) => match self.target.pointer_width() {
                Ok(target_lexicon::PointerWidth::U16) => scalar(Primitive::Int(Integer::I16, true)),
                Ok(target_lexicon::PointerWidth::U32) => scalar(Primitive::Int(Integer::I32, true)),
                Ok(target_lexicon::PointerWidth::U64) => scalar(Primitive::Int(Integer::I64, true)),
                Err(_) => scalar(Primitive::Int(Integer::I32, true)),
            },
            Type::UInt(0) => match self.target.pointer_width() {
                Ok(target_lexicon::PointerWidth::U16) => {
                    scalar(Primitive::Int(Integer::I16, false))
                }
                Ok(target_lexicon::PointerWidth::U32) => {
                    scalar(Primitive::Int(Integer::I32, false))
                }
                Ok(target_lexicon::PointerWidth::U64) => {
                    scalar(Primitive::Int(Integer::I64, false))
                }
                Err(_) => scalar(Primitive::Int(Integer::I32, false)),
            },
            Type::Float(0) => match self.target.pointer_width() {
                Ok(target_lexicon::PointerWidth::U32) => scalar(Primitive::F32),
                Ok(target_lexicon::PointerWidth::U64) => scalar(Primitive::F64),
                _ => scalar(Primitive::F32),
            },
            Type::Int(8) => scalar(Primitive::Int(Integer::I8, true)),
            Type::Int(16) => scalar(Primitive::Int(Integer::I16, true)),
            Type::Int(32) => scalar(Primitive::Int(Integer::I32, true)),
            Type::Int(64) => scalar(Primitive::Int(Integer::I64, true)),
            Type::Int(128) => scalar(Primitive::Int(Integer::I128, true)),
            Type::Int(_) => unreachable!(),
            Type::UInt(8) => scalar(Primitive::Int(Integer::I8, false)),
            Type::UInt(16) => scalar(Primitive::Int(Integer::I16, false)),
            Type::UInt(32) => scalar(Primitive::Int(Integer::I32, false)),
            Type::UInt(64) => scalar(Primitive::Int(Integer::I64, false)),
            Type::UInt(128) => scalar(Primitive::Int(Integer::I128, false)),
            Type::UInt(_) => unreachable!(),
            Type::Float(32) => scalar(Primitive::F32),
            Type::Float(64) => scalar(Primitive::F64),
            Type::Float(_) => unreachable!(),
            Type::Str => {
                let mut data_ptr = scalar_unit(Primitive::Pointer);

                data_ptr.valid_range = 1..=*data_ptr.valid_range.end();

                let metadata = scalar_unit(Primitive::Int(Integer::ptr_sized(self.target), false));

                self.intern_layout(self.scalar_pair(data_ptr, metadata))
            }
            Type::TypeId => match self.target.pointer_width() {
                Ok(target_lexicon::PointerWidth::U16) => {
                    scalar(Primitive::Int(Integer::I16, false))
                }
                Ok(target_lexicon::PointerWidth::U32) => {
                    scalar(Primitive::Int(Integer::I32, false))
                }
                Ok(target_lexicon::PointerWidth::U64) => {
                    scalar(Primitive::Int(Integer::I64, false))
                }
                Err(_) => scalar(Primitive::Int(Integer::I32, false)),
            },
            Type::Ref(_, _) => {
                let data_ptr = scalar_unit(Primitive::Pointer);

                self.intern_layout(Layout::scalar(data_ptr, self.target))
            }
            Type::Array(of, len) => {
                let of_layout = self.layout(of);
                let size = of_layout.stride * (*len as u64);
                let largest_niche = if *len != 0 {
                    of_layout.largest_niche.clone()
                } else {
                    None
                };

                self.intern_layout(Layout {
                    size,
                    align: of_layout.align,
                    stride: size,
                    abi: Abi::Aggregate { sized: true },
                    fields: FieldsShape::Array {
                        stride: of_layout.stride,
                        count: *len as u64,
                    },
                    variants: Variants::Single { index: 0 },
                    largest_niche,
                })
            }
            Type::Slice(_) => {
                let mut data_ptr = scalar_unit(Primitive::Pointer);

                data_ptr.valid_range = 1..=*data_ptr.valid_range.end();

                let metadata = scalar_unit(Primitive::Int(Integer::ptr_sized(self.target), false));

                self.intern_layout(self.scalar_pair(data_ptr, metadata))
            }
            Type::Tuple(tys) => self
                .intern_layout(self.struct_layout(tys.iter().map(|ty| self.layout(ty)).collect())),
            Type::Struct(_, fields) => self.intern_layout(
                self.struct_layout(fields.iter().map(|f| self.layout(f.ty)).collect()),
            ),
            Type::Func(_, _, _) => {
                let mut ptr = scalar_unit(Primitive::Pointer);

                ptr.valid_range = 1..=*ptr.valid_range.end();
                self.intern_layout(Layout::scalar(ptr, self.target))
            }
            Type::Enum(_, variants) => {
                let variants = variants
                    .iter()
                    .map(|v| {
                        self.struct_layout(
                            v.fields
                                .iter()
                                .map(|f| self.layout(f.ty))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();

                self.intern_layout(self.enum_layout(variants))
            }
            Type::Object => {
                let mut data = scalar_unit(Primitive::Pointer);
                let mut meta = scalar_unit(Primitive::Pointer);

                data.valid_range = 1..=*data.valid_range.start();
                meta.valid_range = 1..=*data.valid_range.start();

                self.intern_layout(self.scalar_pair(data, meta))
            }
        };

        let layout = TyLayout { ty, layout };

        self.layouts.borrow_mut().insert(ty as *const _, layout);

        layout
    }

    fn scalar_pair(&self, a: Scalar, b: Scalar) -> Layout {
        let b_align = b.value.align(self.target);
        let b_offset = a.value.size(self.target).align_to(b_align);
        let align = a.value.align(self.target).max(b_align);
        let size = a.value.size(self.target) + b.value.size(self.target);
        let stride = (b_offset + b.value.size(self.target)).align_to(align);
        let largest_niche = Niche::from_scalar(self.target, b_offset, b.clone())
            .into_iter()
            .chain(Niche::from_scalar(self.target, Size::ZERO, a.clone()))
            .max_by_key(|niche| niche.available(self.target));

        Layout {
            fields: FieldsShape::Arbitrary {
                offsets: vec![Size::ZERO, b_offset],
            },
            variants: Variants::Single { index: 0 },
            largest_niche,
            abi: Abi::ScalarPair(a, b),
            align,
            size,
            stride,
        }
    }

    fn struct_layout(&self, fields: Vec<TyLayout<'tcx, Ty<'tcx>>>) -> Layout {
        // TODO: optimize layout
        let mut align = Align::from_bytes(1);
        let mut offsets = vec![Size::ZERO; fields.len()];
        let mut offset = Size::ZERO;
        let mut niches = Vec::new();

        for i in 0..fields.len() {
            let field = fields[i];

            if let Some(niche) = field.largest_niche.clone() {
                niches.push(niche);
            }

            let field_align = field.align;

            offset = offset.align_to(field_align);
            align = align.max(field_align);
            offsets[i] = offset;
            offset = offset + field.size;
        }

        let size = offset;
        let stride = offset.align_to(align);
        let abi = Abi::Aggregate { sized: true };
        let largest_niche = niches
            .into_iter()
            .max_by_key(|niche| niche.available(self.target));

        Layout {
            fields: FieldsShape::Arbitrary { offsets },
            variants: Variants::Single { index: 0 },
            largest_niche,
            abi,
            align,
            size,
            stride,
        }
    }

    fn enum_layout(&self, mut variants: Vec<Layout>) -> Layout {
        if variants.is_empty() {
            Layout {
                fields: FieldsShape::Arbitrary {
                    offsets: Vec::new(),
                },
                variants: Variants::Single { index: 0 },
                largest_niche: None,
                abi: Abi::Aggregate { sized: true },
                size: Size::ZERO,
                align: Align::from_bytes(1),
                stride: Size::ZERO,
            }
        } else if variants.len() == 1 {
            variants.pop().unwrap()
        } else {
            let largest_niche = variants
                .iter()
                .filter_map(|v| v.largest_niche.clone())
                .max_by_key(|niche| niche.available(self.target));

            for (i, variant) in variants.iter_mut().enumerate() {
                variant.variants = Variants::Single { index: i };
            }

            let largest = variants.iter().max_by_key(|v| v.size).unwrap();
            let align = largest.align;
            let mut size = largest.size;
            let mut no_niche = |mut variants: Vec<Layout>| {
                let tag_size = Size::from_bits(variants.len()).align_to(align);
                let offsets = vec![Size::ZERO, tag_size];
                let tag = Scalar {
                    value: Primitive::Int(
                        match tag_size.bytes() {
                            1 => Integer::I8,
                            2 => Integer::I16,
                            4 => Integer::I32,
                            8 => Integer::I64,
                            _ => Integer::I128,
                        },
                        false,
                    ),
                    valid_range: 0..=u128::max_value(),
                };

                let tag_encoding = TagEncoding::Direct;

                size = size + tag_size;

                for variant in &mut variants {
                    if let FieldsShape::Arbitrary { offsets } = &mut variant.fields {
                        for offset in offsets {
                            *offset = *offset + tag_size;
                        }
                    }
                }

                (
                    FieldsShape::Arbitrary { offsets },
                    Variants::Multiple {
                        tag,
                        tag_encoding,
                        tag_field: 0,
                        variants,
                    },
                )
            };

            let (fields, variants) = if let Some(niche) = largest_niche {
                if niche.available(self.target) >= variants.len() as u128 {
                    // unimplemented!();
                    no_niche(variants) // TODO: implement niches
                } else {
                    no_niche(variants)
                }
            } else {
                no_niche(variants)
            };

            let stride = size.align_to(align);

            Layout {
                fields,
                variants,
                largest_niche: None,
                abi: Abi::Aggregate { sized: true },
                size,
                align,
                stride,
            }
        }
    }

    pub fn intern_ty(&self, ty: Type<'tcx>) -> Ty<'tcx> {
        self.intern.intern_ty(ty)
    }

    pub(crate) fn intern_layout(&self, layout: Layout) -> &'tcx Layout {
        self.intern.intern_layout(layout)
    }
}

struct Interned<'tcx, T: ?Sized>(&'tcx T);

impl<'tcx, T: 'tcx + ?Sized> Clone for Interned<'tcx, T> {
    fn clone(&self) -> Self {
        Interned(self.0)
    }
}
impl<'tcx, T: 'tcx + ?Sized> Copy for Interned<'tcx, T> {}

impl<'tcx> PartialEq for Interned<'tcx, Type<'tcx>> {
    fn eq(&self, other: &Interned<'tcx, Type<'tcx>>) -> bool {
        self.0 == other.0
    }
}

impl<'tcx> Eq for Interned<'tcx, Type<'tcx>> {}

impl<'tcx> std::hash::Hash for Interned<'tcx, Type<'tcx>> {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.0.hash(s)
    }
}

impl<'tcx> std::borrow::Borrow<Type<'tcx>> for Interned<'tcx, Type<'tcx>> {
    fn borrow<'a>(&'a self) -> &'a Type<'tcx> {
        self.0
    }
}

impl<'tcx> PartialEq for Interned<'tcx, Layout> {
    fn eq(&self, other: &Interned<'tcx, Layout>) -> bool {
        self.0 == other.0
    }
}

impl<'tcx> Eq for Interned<'tcx, Layout> {}

impl<'tcx> std::hash::Hash for Interned<'tcx, Layout> {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.0.hash(s)
    }
}

impl<'tcx> std::borrow::Borrow<Layout> for Interned<'tcx, Layout> {
    fn borrow<'a>(&'a self) -> &'a Layout {
        self.0
    }
}

impl<'tcx, T: PartialEq> PartialEq for Interned<'tcx, List<T>> {
    fn eq(&self, other: &Interned<'tcx, List<T>>) -> bool {
        self.0[..] == other.0[..]
    }
}

impl<'tcx, T: Eq> Eq for Interned<'tcx, List<T>> {}

impl<'tcx, T: std::hash::Hash> std::hash::Hash for Interned<'tcx, List<T>> {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.0[..].hash(s)
    }
}

impl<'tcx, T> std::borrow::Borrow<[T]> for Interned<'tcx, List<T>> {
    fn borrow<'a>(&'a self) -> &'a [T] {
        &self.0[..]
    }
}