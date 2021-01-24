use crate::ast::*;

impl Module {
    pub fn decl_groups(&self) -> DeclGroups {
        DeclGroups { decls: &self.decls, start: 0 }
    }
}

pub struct DeclGroups<'ast> {
    decls: &'ast [Decl],
    start: usize,
}

#[derive(Clone, Copy)]
pub enum DeclGroupKind {
    Foreign,
    Func(bool),
    Const(bool),
    Static(bool),
    Fixity,
    Type(bool),
    Class,
    Instance,
}

pub struct InstanceDeclGroup<'ast> {
    decls: &'ast [InstanceDecl],
    start: usize,
}

#[derive(Clone, Copy)]
pub enum InstanceDeclGroupKind {
    Func(bool),
}

pub struct LetBindingGroups<'ast> {
    bindings: &'ast [LetBinding],
    start: usize,
}

impl<'ast> InstanceDeclGroup<'ast> {
    pub fn new(decls: &'ast [InstanceDecl]) -> Self {
        InstanceDeclGroup { decls, start: 0 }
    }
}

impl<'ast> LetBindingGroups<'ast> {
    pub fn new(bindings: &'ast [LetBinding]) -> Self {
        LetBindingGroups { bindings, start: 0 }
    }
}

impl Decl {
    fn group_kind(&self) -> DeclGroupKind {
        match &self.kind {
            | DeclKind::Foreign { .. } => DeclGroupKind::Foreign,
            | DeclKind::FuncTy { .. } => DeclGroupKind::Func(true),
            | DeclKind::Func { .. } => DeclGroupKind::Func(false),
            | DeclKind::ConstTy { .. } => DeclGroupKind::Const(true),
            | DeclKind::Const { .. } => DeclGroupKind::Const(false),
            | DeclKind::StaticTy { .. } => DeclGroupKind::Static(true),
            | DeclKind::Static { .. } => DeclGroupKind::Static(false),
            | DeclKind::Fixity { .. } => DeclGroupKind::Fixity,
            | DeclKind::TypeKind { .. } => DeclGroupKind::Type(true),
            | DeclKind::Alias { .. } => DeclGroupKind::Type(false),
            | DeclKind::Data { .. } => DeclGroupKind::Type(false),
            | DeclKind::Class { .. } => DeclGroupKind::Class,
            | DeclKind::InstanceChain { .. } => DeclGroupKind::Instance,
        }
    }
}

impl DeclGroupKind {
    fn max(&self) -> usize {
        match self {
            | DeclGroupKind::Foreign => 1,
            | DeclGroupKind::Func(_) => usize::max_value(),
            | DeclGroupKind::Const(_) => 2,
            | DeclGroupKind::Static(_) => 2,
            | DeclGroupKind::Fixity => 1,
            | DeclGroupKind::Type(_) => 2,
            | DeclGroupKind::Class => 1,
            | DeclGroupKind::Instance => 1,
        }
    }
}

impl PartialEq for DeclGroupKind {
    fn eq(&self, other: &Self) -> bool {
        use DeclGroupKind::*;

        match (self, other) {
            | (Foreign, Foreign) => true,
            | (Func(true), Func(false)) => true,
            | (Func(false), Func(false)) => true,
            | (Const(true), Const(false)) => true,
            | (Static(true), Static(false)) => true,
            | (Type(true), Type(false)) => true,
            | (Class, Class) => true,
            | (Instance, Instance) => true,
            | _ => false,
        }
    }
}

impl InstanceDecl {
    fn group_kind(&self) -> InstanceDeclGroupKind {
        match &self.kind {
            | InstanceDeclKind::FuncTy { .. } => InstanceDeclGroupKind::Func(true),
            | InstanceDeclKind::Func { .. } => InstanceDeclGroupKind::Func(false),
        }
    }
}

impl InstanceDeclGroupKind {
    fn max(&self) -> usize {
        match self {
            | InstanceDeclGroupKind::Func(_) => usize::max_value(),
        }
    }
}

impl PartialEq for InstanceDeclGroupKind {
    fn eq(&self, other: &Self) -> bool {
        use InstanceDeclGroupKind::*;

        match (self, other) {
            | (Func(true), Func(false)) => true,
            | (Func(false), Func(false)) => true,
            | _ => false,
        }
    }
}

impl<'ast> Iterator for DeclGroups<'ast> {
    type Item = (DeclGroupKind, &'ast [Decl]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.decls.len() {
            return None;
        }

        let mut pos = self.start;
        let name = self.decls[pos].name.symbol;
        let mut kind = self.decls[pos].group_kind();

        pos += 1;

        while pos < self.decls.len() && self.decls[pos].name.symbol == name && pos - self.start < kind.max() {
            let kind2 = self.decls[pos].group_kind();

            if kind == kind2 {
                kind = kind2;
                pos += 1;
            } else {
                break;
            }
        }

        let start = std::mem::replace(&mut self.start, pos);

        Some((kind, &self.decls[start..pos]))
    }
}

impl<'ast> Iterator for InstanceDeclGroup<'ast> {
    type Item = (InstanceDeclGroupKind, &'ast [InstanceDecl]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.decls.len() {
            return None;
        }

        let mut pos = self.start;
        let name = self.decls[pos].name.symbol;
        let kind = self.decls[pos].group_kind();

        pos += 1;

        while pos < self.decls.len() && self.decls[pos].name.symbol == name && pos - self.start < kind.max() {
            let kind2 = self.decls[pos].group_kind();

            if kind == kind2 {
                pos += 1;
            } else {
                break;
            }
        }

        let start = std::mem::replace(&mut self.start, pos);

        Some((kind, &self.decls[start..pos]))
    }
}

impl<'ast> Iterator for LetBindingGroups<'ast> {
    type Item = &'ast [LetBinding];

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.bindings.len() {
            return None;
        }

        let start = self.start;
        let end;

        match self.bindings[self.start].kind {
            | LetBindingKind::Type { .. } if self.start + 1 < self.bindings.len() => match self.bindings[self.start + 1].kind {
                | LetBindingKind::Type { .. } => end = start + 1,
                | LetBindingKind::Value { .. } => end = start + 2,
            },
            | LetBindingKind::Type { .. } => end = start + 1,
            | LetBindingKind::Value { .. } => end = start + 1,
        }

        self.start = end;

        Some(&self.bindings[start..end])
    }
}
