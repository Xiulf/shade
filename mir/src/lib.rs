#![feature(drain_filter)]

pub mod constant;
pub mod convert;
pub mod optimize;
mod printing;
pub mod visit;

pub use check::ty::{Ident, Param, Ty, Type};
pub use hir::{Id, ItemId};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Package<'tcx> {
    pub items: BTreeMap<Id, Item<'tcx>>,
}

#[derive(Debug, Clone)]
pub struct Item<'tcx> {
    pub id: Id,
    pub name: Ident,
    pub kind: ItemKind<'tcx>,
}

#[derive(Debug, Clone)]
pub enum ItemKind<'tcx> {
    Extern(Ty<'tcx>),
    Global(Ty<'tcx>, Const<'tcx>),
    Body(Body<'tcx>),
}

#[derive(Debug, Clone)]
pub struct Body<'tcx> {
    pub locals: BTreeMap<LocalId, Local<'tcx>>,
    pub blocks: BTreeMap<BlockId, Block<'tcx>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Local<'tcx> {
    pub id: LocalId,
    pub kind: LocalKind,
    pub ty: Ty<'tcx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LocalKind {
    Ret,
    Arg,
    Var,
    Tmp,
}

#[derive(Debug, Clone)]
pub struct Block<'tcx> {
    pub id: BlockId,
    pub stmts: Vec<Stmt<'tcx>>,
    pub term: Term<'tcx>,
}

#[derive(Debug, Clone)]
pub enum Stmt<'tcx> {
    Nop,
    Assign(Place, RValue<'tcx>),
}

#[derive(Debug, Clone)]
pub enum Term<'tcx> {
    Unset,
    Abort,
    Return,
    Jump(BlockId),
    Switch(Operand<'tcx>, Vec<u128>, Vec<BlockId>),
    Call(Place, Operand<'tcx>, Vec<Operand<'tcx>>, BlockId),
}

#[derive(Debug, Clone)]
pub struct Place {
    pub base: PlaceBase,
    pub elems: Vec<PlaceElem>,
}

#[derive(Debug, Clone)]
pub enum PlaceBase {
    Local(LocalId),
    Global(Id),
}

#[derive(Debug, Clone)]
pub enum PlaceElem {
    Deref,
    Field(usize),
    Index(Place),
}

#[derive(Debug, Clone)]
pub enum Operand<'tcx> {
    Place(Place),
    Const(Const<'tcx>),
}

#[derive(Debug, Clone)]
pub enum Const<'tcx> {
    Undefined,
    Tuple(Vec<Const<'tcx>>),
    Array(Vec<Const<'tcx>>),
    Scalar(u128, Ty<'tcx>),
    FuncAddr(Id),
    Bytes(Box<[u8]>),
    Type(Ty<'tcx>),
}

#[derive(Debug, Clone)]
pub enum RValue<'tcx> {
    Use(Operand<'tcx>),
    Ref(Place),
    Cast(Ty<'tcx>, Operand<'tcx>),
    BinOp(BinOp, Operand<'tcx>, Operand<'tcx>),
    UnOp(UnOp, Operand<'tcx>),
    Init(Ty<'tcx>, Vec<Operand<'tcx>>),
}

#[derive(Debug, Clone)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    BitAnd,
    BitOr,
    BitXOr,
    Shl,
    Shr,
}

#[derive(Debug, Clone)]
pub enum UnOp {
    Neg,
    Not,
}

impl<'tcx> Package<'tcx> {
    pub fn new() -> Self {
        Package {
            items: BTreeMap::new(),
        }
    }

    pub fn declare_extern(&mut self, id: Id, name: Ident, ty: Ty<'tcx>) {
        self.items.insert(
            id,
            Item {
                id,
                name,
                kind: ItemKind::Extern(ty),
            },
        );
    }

    pub fn declare_global(&mut self, id: Id, name: Ident, ty: Ty<'tcx>) {
        self.items.insert(
            id,
            Item {
                id,
                name,
                kind: ItemKind::Global(ty, Const::Undefined),
            },
        );
    }

    pub fn declare_body(&mut self, id: Id, name: Ident, params: &[Param<'tcx>], ret: Ty<'tcx>) {
        let mut locals = BTreeMap::new();

        locals.insert(
            LocalId(0),
            Local {
                id: LocalId(0),
                kind: LocalKind::Ret,
                ty: ret,
            },
        );

        for param in params {
            let id = LocalId(locals.len());

            locals.insert(
                id,
                Local {
                    id,
                    kind: LocalKind::Arg,
                    ty: param.ty,
                },
            );
        }

        self.items.insert(
            id,
            Item {
                id,
                name,
                kind: ItemKind::Body(Body {
                    locals,
                    blocks: BTreeMap::new(),
                }),
            },
        );
    }

    pub fn define_global(&mut self, id: Id, f: impl FnOnce(Builder<'_, 'tcx>)) {
        let ty = match self.items.get(&id).unwrap().kind {
            ItemKind::Global(ty, _) => ty,
            _ => unreachable!(),
        };

        let mut body = Body {
            locals: BTreeMap::new(),
            blocks: BTreeMap::new(),
        };

        body.locals.insert(
            LocalId::RET,
            Local {
                id: LocalId::RET,
                ty,
                kind: LocalKind::Ret,
            },
        );

        let builder = Builder {
            body: &mut body,
            current_block: None,
        };

        f(builder);

        let value = crate::constant::eval(body, self);

        if let ItemKind::Global(_, val) = &mut self.items.get_mut(&id).unwrap().kind {
            *val = value;
        }
    }

    pub fn define_body<'a>(&'a mut self, id: Id) -> Builder<'a, 'tcx> {
        if let ItemKind::Body(body) = &mut self.items.get_mut(&id).unwrap().kind {
            Builder {
                body,
                current_block: None,
            }
        } else {
            panic!("item is not a body");
        }
    }
}

impl<'tcx> Body<'tcx> {
    pub fn params(&self) -> impl Iterator<Item = &Local<'tcx>> {
        self.locals.values().filter(|v| v.kind == LocalKind::Arg)
    }
}

impl LocalId {
    pub const RET: Self = LocalId(0);

    pub const fn as_u32(self) -> u32 {
        self.0 as u32
    }
}

impl Place {
    pub fn local(id: LocalId) -> Self {
        Place {
            base: PlaceBase::Local(id),
            elems: Vec::new(),
        }
    }

    pub fn global(id: Id) -> Self {
        Place {
            base: PlaceBase::Global(id),
            elems: Vec::new(),
        }
    }

    pub fn deref(mut self) -> Self {
        self.elems.push(PlaceElem::Deref);
        self
    }

    pub fn field(mut self, idx: usize) -> Self {
        self.elems.push(PlaceElem::Field(idx));
        self
    }

    pub fn index(mut self, idx: Place) -> Self {
        self.elems.push(PlaceElem::Index(idx));
        self
    }
}

pub struct Builder<'a, 'tcx> {
    pub body: &'a mut Body<'tcx>,
    current_block: Option<BlockId>,
}

impl<'a, 'tcx> Builder<'a, 'tcx> {
    fn block(&mut self) -> &mut Block<'tcx> {
        self.body
            .blocks
            .get_mut(self.current_block.as_ref().unwrap())
            .unwrap()
    }

    pub fn current_block(&self) -> BlockId {
        self.current_block.unwrap()
    }

    pub fn create_var(&mut self, ty: Ty<'tcx>) -> LocalId {
        let id = LocalId(self.body.locals.len());

        self.body.locals.insert(
            id,
            Local {
                id,
                kind: LocalKind::Var,
                ty,
            },
        );

        id
    }

    pub fn create_tmp(&mut self, ty: Ty<'tcx>) -> LocalId {
        let id = LocalId(self.body.locals.len());

        self.body.locals.insert(
            id,
            Local {
                id,
                kind: LocalKind::Tmp,
                ty,
            },
        );

        id
    }

    pub fn create_block(&mut self) -> BlockId {
        let id = BlockId(self.body.blocks.len());

        self.body.blocks.insert(
            id,
            Block {
                id,
                stmts: Vec::new(),
                term: Term::Unset,
            },
        );

        id
    }

    pub fn placed(&mut self, op: Operand<'tcx>, ty: Ty<'tcx>) -> Place {
        match op {
            Operand::Place(place) => place,
            _ => {
                let tmp = self.create_tmp(ty);
                let place = Place::local(tmp);

                self.use_(place.clone(), op);
                place
            }
        }
    }

    pub fn use_block(&mut self, id: BlockId) {
        self.current_block = Some(id);
    }

    pub fn use_(&mut self, place: Place, op: Operand<'tcx>) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::Use(op)));
    }

    pub fn ref_(&mut self, place: Place, to: Place) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::Ref(to)));
    }

    pub fn cast(&mut self, place: Place, ty: Ty<'tcx>, op: Operand<'tcx>) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::Cast(ty, op)));
    }

    pub fn binop(&mut self, place: Place, op: BinOp, lhs: Operand<'tcx>, rhs: Operand<'tcx>) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::BinOp(op, lhs, rhs)));
    }

    pub fn unop(&mut self, place: Place, op: UnOp, rhs: Operand<'tcx>) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::UnOp(op, rhs)));
    }

    pub fn init(&mut self, place: Place, ty: Ty<'tcx>, ops: Vec<Operand<'tcx>>) {
        self.block()
            .stmts
            .push(Stmt::Assign(place, RValue::Init(ty, ops)));
    }

    pub fn abort(&mut self) {
        if let Term::Unset = self.block().term {
            self.block().term = Term::Abort;
        }
    }

    pub fn return_(&mut self) {
        if let Term::Unset = self.block().term {
            self.block().term = Term::Return;
        }
    }

    pub fn jump(&mut self, target: BlockId) {
        if let Term::Unset = self.block().term {
            self.block().term = Term::Jump(target);
        }
    }

    pub fn switch(&mut self, op: Operand<'tcx>, vals: Vec<u128>, targets: Vec<BlockId>) {
        if let Term::Unset = self.block().term {
            self.block().term = Term::Switch(op, vals, targets);
        }
    }

    pub fn call(
        &mut self,
        place: Place,
        func: Operand<'tcx>,
        args: Vec<Operand<'tcx>>,
        target: BlockId,
    ) {
        if let Term::Unset = self.block().term {
            self.block().term = Term::Call(place, func, args, target);
        }
    }
}
