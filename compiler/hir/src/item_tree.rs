mod lower;

use crate::arena::{Arena, Idx};
use crate::ast_id::FileAstId;
use crate::db::DefDatabase;
use crate::in_file::InFile;
use crate::name::Name;
use crate::path::ModPath;
use base_db::input::FileId;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Index;
use std::sync::Arc;
use syntax::ast::{self, AstNode};

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemTree {
    top_level: Vec<Item>,
    data: ItemTreeData,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ItemTreeData {
    imports: Arena<Import>,
    fixities: Arena<Fixity>,
    foreigns: Arena<Foreign>,
    funcs: Arena<Func>,
    statics: Arena<Static>,
    consts: Arena<Const>,
    types: Arena<Type>,
    classes: Arena<Class>,
    instances: Arena<Instance>,
}

pub trait ItemTreeNode: Clone {
    type Source: AstNode + Into<ast::Item>;

    fn ast_id(&self) -> FileAstId<Self::Source>;

    fn lookup(tree: &ItemTree, index: Idx<Self>) -> &Self;

    fn id_from_item(item: Item) -> Option<LocalItemTreeId<Self>>;

    fn id_to_item(id: LocalItemTreeId<Self>) -> Item;
}

pub struct LocalItemTreeId<N: ItemTreeNode> {
    index: Idx<N>,
    _marker: PhantomData<N>,
}

pub type ItemTreeId<N> = InFile<LocalItemTreeId<N>>;

impl ItemTree {
    pub fn item_tree_query(db: &dyn DefDatabase, file_id: FileId) -> Arc<ItemTree> {
        let syntax = db.parse(file_id);
        let ctx = lower::Ctx::new(db, file_id);
        let item_tree = ctx.lower_items(&syntax.tree());

        Arc::new(item_tree)
    }

    pub fn top_level(&self) -> &[Item] {
        &self.top_level
    }
}

impl<N: ItemTreeNode> Index<LocalItemTreeId<N>> for ItemTree {
    type Output = N;

    fn index(&self, id: LocalItemTreeId<N>) -> &N {
        N::lookup(self, id.index)
    }
}

macro_rules! items {
    ($($typ:ident in $f_id:ident -> $ast:ty),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Item {
            $($typ(LocalItemTreeId<$typ>),)*
        }

        $(
            impl From<LocalItemTreeId<$typ>> for Item {
                fn from(id: LocalItemTreeId<$typ>) -> Self {
                    Item::$typ(id)
                }
            }
        )*

        $(
            impl ItemTreeNode for $typ {
                type Source = $ast;

                fn ast_id(&self) -> FileAstId<Self::Source> {
                    self.ast_id
                }

                fn lookup(tree: &ItemTree, index: Idx<Self>) -> &Self {
                    &tree.data.$f_id[index]
                }

                fn id_from_item(item: Item) -> Option<LocalItemTreeId<Self>> {
                    if let Item::$typ(id) = item {
                        Some(id)
                    } else {
                        None
                    }
                }

                fn id_to_item(id: LocalItemTreeId<Self>) -> Item {
                    Item::$typ(id)
                }
            }

            impl Index<Idx<$typ>> for ItemTree {
                type Output = $typ;

                fn index(&self, index: Idx<$typ>) -> &Self::Output {
                    &self.data.$f_id[index]
                }
            }
        )*
    }
}

items! {
    Import in imports -> ast::ItemImport,
    Fixity in fixities -> ast::ItemFixity,
    Foreign in foreigns -> ast::ItemForeign,
    Func in funcs -> ast::ItemFun,
    Static in statics -> ast::ItemStatic,
    Const in consts -> ast::ItemConst,
    Type in types -> ast::ItemType,
    Class in classes -> ast::ItemClass,
    Instance in instances -> ast::ItemInstance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub ast_id: FileAstId<ast::ItemImport>,
    pub path: ModPath,
    pub alias: Option<Name>,
    pub is_glob: bool,
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixity {
    pub ast_id: FileAstId<ast::ItemFixity>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreign {
    pub ast_id: FileAstId<ast::ItemForeign>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Func {
    pub ast_id: FileAstId<ast::ItemFun>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Static {
    pub ast_id: FileAstId<ast::ItemStatic>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Const {
    pub ast_id: FileAstId<ast::ItemConst>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub ast_id: FileAstId<ast::ItemType>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    pub ast_id: FileAstId<ast::ItemClass>,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub ast_id: FileAstId<ast::ItemInstance>,
}

impl<N: ItemTreeNode> Clone for LocalItemTreeId<N> {
    fn clone(&self) -> Self {
        LocalItemTreeId {
            index: self.index,
            _marker: PhantomData,
        }
    }
}

impl<N: ItemTreeNode> Copy for LocalItemTreeId<N> {
}

impl<N: ItemTreeNode> PartialEq for LocalItemTreeId<N> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<N: ItemTreeNode> Eq for LocalItemTreeId<N> {
}

impl<N: ItemTreeNode> Hash for LocalItemTreeId<N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}

impl<N: ItemTreeNode> fmt::Debug for LocalItemTreeId<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.index.fmt(f)
    }
}