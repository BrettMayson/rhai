//! Module implementing the AST optimizer.

use crate::any::Dynamic;
use crate::engine::{
    Engine, KEYWORD_DEBUG, KEYWORD_EVAL, KEYWORD_IS_DEF_FN, KEYWORD_IS_DEF_VAR, KEYWORD_PRINT,
    KEYWORD_TYPE_OF,
};
use crate::fn_call::run_builtin_binary_op;
use crate::module::Module;
use crate::parser::{map_dynamic_to_expr, BinaryExpr, Expr, ScriptFnDef, Stmt, AST};
use crate::scope::{Entry as ScopeEntry, Scope};
use crate::token::{is_valid_identifier, Position};
use crate::{calc_fn_hash, StaticVec};

#[cfg(not(feature = "no_function"))]
use crate::parser::ReturnType;

#[cfg(feature = "internals")]
use crate::parser::CustomExpr;

use crate::stdlib::{
    boxed::Box,
    iter::empty,
    string::{String, ToString},
    vec,
    vec::Vec,
};

/// Level of optimization performed.
///
/// Not available under the `no_optimize` feature.
#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum OptimizationLevel {
    /// No optimization performed.
    None,
    /// Only perform simple optimizations without evaluating functions.
    Simple,
    /// Full optimizations performed, including evaluating functions.
    /// Take care that this may cause side effects as it essentially assumes that all functions are pure.
    Full,
}

impl OptimizationLevel {
    /// Is the `OptimizationLevel` None.
    #[inline(always)]
    pub fn is_none(self) -> bool {
        self == Self::None
    }
    /// Is the `OptimizationLevel` Simple.
    #[cfg(not(feature = "no_optimize"))]
    #[inline(always)]
    pub fn is_simple(self) -> bool {
        self == Self::Simple
    }
    /// Is the `OptimizationLevel` Full.
    #[cfg(not(feature = "no_optimize"))]
    #[inline(always)]
    pub fn is_full(self) -> bool {
        self == Self::Full
    }
}

/// Mutable state throughout an optimization pass.
#[derive(Debug, Clone)]
struct State<'a> {
    /// Has the AST been changed during this pass?
    changed: bool,
    /// Collection of constants to use for eager function evaluations.
    constants: Vec<(String, Expr)>,
    /// An `Engine` instance for eager function evaluation.
    engine: &'a Engine,
    /// Library of script-defined functions.
    lib: &'a [&'a Module],
    /// Optimization level.
    optimization_level: OptimizationLevel,
}

impl<'a> State<'a> {
    /// Create a new State.
    #[inline(always)]
    pub fn new(engine: &'a Engine, lib: &'a [&'a Module], level: OptimizationLevel) -> Self {
        Self {
            changed: false,
            constants: vec![],
            engine,
            lib,
            optimization_level: level,
        }
    }
    /// Reset the state from dirty to clean.
    #[inline(always)]
    pub fn reset(&mut self) {
        self.changed = false;
    }
    /// Set the AST state to be dirty (i.e. changed).
    #[inline(always)]
    pub fn set_dirty(&mut self) {
        self.changed = true;
    }
    /// Is the AST dirty (i.e. changed)?
    #[inline(always)]
    pub fn is_dirty(&self) -> bool {
        self.changed
    }
    /// Does a constant exist?
    #[inline(always)]
    pub fn contains_constant(&self, name: &str) -> bool {
        self.constants.iter().any(|(n, _)| n == name)
    }
    /// Prune the list of constants back to a specified size.
    #[inline(always)]
    pub fn restore_constants(&mut self, len: usize) {
        self.constants.truncate(len)
    }
    /// Add a new constant to the list.
    #[inline(always)]
    pub fn push_constant(&mut self, name: &str, value: Expr) {
        self.constants.push((name.into(), value))
    }
    /// Look up a constant from the list.
    #[inline]
    pub fn find_constant(&self, name: &str) -> Option<&Expr> {
        for (n, expr) in self.constants.iter().rev() {
            if n == name {
                return Some(expr);
            }
        }

        None
    }
}

/// Call a registered function
fn call_fn_with_constant_arguments(
    state: &State,
    fn_name: &str,
    arg_values: &mut [Dynamic],
) -> Option<Dynamic> {
    // Search built-in's and external functions
    let hash_fn = calc_fn_hash(
        empty(),
        fn_name,
        arg_values.len(),
        arg_values.iter().map(|a| a.type_id()),
    );

    state
        .engine
        .call_native_fn(
            &mut Default::default(),
            state.lib,
            fn_name,
            hash_fn,
            arg_values.iter_mut().collect::<StaticVec<_>>().as_mut(),
            false,
            true,
            &None,
        )
        .ok()
        .map(|(v, _)| v)
}

/// Optimize a statement.
fn optimize_stmt(stmt: Stmt, state: &mut State, preserve_result: bool) -> Stmt {
    match stmt {
        // id op= expr
        Stmt::Assignment(x, pos) => Stmt::Assignment(
            Box::new((optimize_expr(x.0, state), x.1, optimize_expr(x.2, state))),
            pos,
        ),
        // if false { if_block } -> Noop
        Stmt::IfThenElse(Expr::False(pos), x, _) if x.1.is_none() => {
            state.set_dirty();
            Stmt::Noop(pos)
        }
        // if true { if_block } -> if_block
        Stmt::IfThenElse(Expr::True(_), x, _) if x.1.is_none() => optimize_stmt(x.0, state, true),
        // if expr { Noop }
        Stmt::IfThenElse(condition, x, _) if x.1.is_none() && matches!(x.0, Stmt::Noop(_)) => {
            state.set_dirty();

            let pos = condition.position();
            let expr = optimize_expr(condition, state);

            if preserve_result {
                // -> { expr, Noop }
                let mut statements = Vec::new();
                statements.push(Stmt::Expr(expr));
                statements.push(x.0);

                Stmt::Block(statements, pos)
            } else {
                // -> expr
                Stmt::Expr(expr)
            }
        }
        // if expr { if_block }
        Stmt::IfThenElse(condition, x, pos) if x.1.is_none() => Stmt::IfThenElse(
            optimize_expr(condition, state),
            Box::new((optimize_stmt(x.0, state, true), None)),
            pos,
        ),
        // if false { if_block } else { else_block } -> else_block
        Stmt::IfThenElse(Expr::False(_), x, _) if x.1.is_some() => {
            optimize_stmt(x.1.unwrap(), state, true)
        }
        // if true { if_block } else { else_block } -> if_block
        Stmt::IfThenElse(Expr::True(_), x, _) => optimize_stmt(x.0, state, true),
        // if expr { if_block } else { else_block }
        Stmt::IfThenElse(condition, x, pos) => Stmt::IfThenElse(
            optimize_expr(condition, state),
            Box::new((
                optimize_stmt(x.0, state, true),
                match optimize_stmt(x.1.unwrap(), state, true) {
                    Stmt::Noop(_) => None, // Noop -> no else block
                    stmt => Some(stmt),
                },
            )),
            pos,
        ),

        // while false { block } -> Noop
        Stmt::While(Expr::False(pos), _, _) => {
            state.set_dirty();
            Stmt::Noop(pos)
        }
        // while true { block } -> loop { block }
        Stmt::While(Expr::True(_), block, pos) => {
            Stmt::Loop(Box::new(optimize_stmt(*block, state, false)), pos)
        }
        // while expr { block }
        Stmt::While(condition, block, pos) => {
            match optimize_stmt(*block, state, false) {
                // while expr { break; } -> { expr; }
                Stmt::Break(pos) => {
                    // Only a single break statement - turn into running the guard expression once
                    state.set_dirty();
                    let mut statements = Vec::new();
                    statements.push(Stmt::Expr(optimize_expr(condition, state)));
                    if preserve_result {
                        statements.push(Stmt::Noop(pos))
                    }
                    Stmt::Block(statements, pos)
                }
                // while expr { block }
                stmt => Stmt::While(optimize_expr(condition, state), Box::new(stmt), pos),
            }
        }
        // loop { block }
        Stmt::Loop(block, pos) => match optimize_stmt(*block, state, false) {
            // loop { break; } -> Noop
            Stmt::Break(pos) => {
                // Only a single break statement
                state.set_dirty();
                Stmt::Noop(pos)
            }
            // loop { block }
            stmt => Stmt::Loop(Box::new(stmt), pos),
        },
        // for id in expr { block }
        Stmt::For(iterable, x, pos) => {
            let (var_name, block) = *x;
            Stmt::For(
                optimize_expr(iterable, state),
                Box::new((var_name, optimize_stmt(block, state, false))),
                pos,
            )
        }
        // let id = expr;
        Stmt::Let(name, Some(expr), pos) => Stmt::Let(name, Some(optimize_expr(expr, state)), pos),
        // let id;
        stmt @ Stmt::Let(_, None, _) => stmt,
        // import expr as var;
        #[cfg(not(feature = "no_module"))]
        Stmt::Import(expr, alias, pos) => Stmt::Import(optimize_expr(expr, state), alias, pos),
        // { block }
        Stmt::Block(statements, pos) => {
            let orig_len = statements.len(); // Original number of statements in the block, for change detection
            let orig_constants_len = state.constants.len(); // Original number of constants in the state, for restore later

            // Optimize each statement in the block
            let mut result: Vec<_> = statements
                .into_iter()
                .map(|stmt| match stmt {
                    // Add constant literals into the state
                    Stmt::Const(name, Some(expr), pos) if expr.is_literal() => {
                        state.set_dirty();
                        state.push_constant(&name.0, expr);
                        Stmt::Noop(pos) // No need to keep constants
                    }
                    Stmt::Const(name, Some(expr), pos) if expr.is_literal() => {
                        let expr = optimize_expr(expr, state);
                        Stmt::Const(name, Some(expr), pos)
                    }
                    Stmt::Const(name, None, pos) => {
                        state.set_dirty();
                        state.push_constant(&name.0, Expr::Unit(name.1));
                        Stmt::Noop(pos) // No need to keep constants
                    }
                    // Optimize the statement
                    stmt => optimize_stmt(stmt, state, preserve_result),
                })
                .collect();

            // Remove all raw expression statements that are pure except for the very last statement
            let last_stmt = if preserve_result { result.pop() } else { None };

            result.retain(|stmt| !stmt.is_pure());

            if let Some(stmt) = last_stmt {
                result.push(stmt);
            }

            // Remove all let/import statements at the end of a block - the new variables will go away anyway.
            // But be careful only remove ones that have no initial values or have values that are pure expressions,
            // otherwise there may be side effects.
            let mut removed = false;

            while let Some(expr) = result.pop() {
                match expr {
                    Stmt::Let(_, expr, _) => {
                        removed = expr.as_ref().map(Expr::is_pure).unwrap_or(true)
                    }
                    #[cfg(not(feature = "no_module"))]
                    Stmt::Import(expr, _, _) => removed = expr.is_pure(),
                    _ => {
                        result.push(expr);
                        break;
                    }
                }
            }

            if preserve_result {
                if removed {
                    result.push(Stmt::Noop(pos))
                }

                // Optimize all the statements again
                result = result
                    .into_iter()
                    .rev()
                    .enumerate()
                    .map(|(i, stmt)| optimize_stmt(stmt, state, i == 0))
                    .rev()
                    .collect();
            }

            // Remove everything following the the first return/throw
            let mut dead_code = false;

            result.retain(|stmt| {
                if dead_code {
                    return false;
                }

                match stmt {
                    Stmt::ReturnWithVal(_, _, _) | Stmt::Break(_) => dead_code = true,
                    _ => (),
                }

                true
            });

            // Change detection
            if orig_len != result.len() {
                state.set_dirty();
            }

            // Pop the stack and remove all the local constants
            state.restore_constants(orig_constants_len);

            match &result[..] {
                // No statements in block - change to No-op
                [] => {
                    state.set_dirty();
                    Stmt::Noop(pos)
                }
                // Only one let statement - leave it alone
                [x] if matches!(x, Stmt::Let(_, _, _)) => Stmt::Block(result, pos),
                // Only one import statement - leave it alone
                #[cfg(not(feature = "no_module"))]
                [x] if matches!(x, Stmt::Import(_, _, _)) => Stmt::Block(result, pos),
                // Only one statement - promote
                [_] => {
                    state.set_dirty();
                    result.remove(0)
                }
                _ => Stmt::Block(result, pos),
            }
        }
        // try { block } catch ( var ) { block }
        Stmt::TryCatch(x) if (x.0).0.is_pure() => {
            // If try block is pure, there will never be any exceptions
            state.set_dirty();
            let pos = (x.0).0.position();
            let mut statements: Vec<_> = Default::default();
            statements.push(optimize_stmt((x.0).0, state, preserve_result));
            statements.push(Stmt::Noop(pos));
            Stmt::Block(statements, pos)
        }
        // try { block } catch ( var ) { block }
        Stmt::TryCatch(x) => {
            let ((try_block, try_pos), var_name, (catch_block, catch_pos)) = *x;
            Stmt::TryCatch(Box::new((
                (optimize_stmt(try_block, state, false), try_pos),
                var_name,
                (optimize_stmt(catch_block, state, false), catch_pos),
            )))
        }
        // expr;
        Stmt::Expr(Expr::Stmt(x)) if matches!(x.0, Stmt::Expr(_)) => {
            state.set_dirty();
            optimize_stmt(x.0, state, preserve_result)
        }
        // expr;
        Stmt::Expr(expr) => Stmt::Expr(optimize_expr(expr, state)),
        // return expr;
        Stmt::ReturnWithVal(ret, Some(expr), pos) => {
            Stmt::ReturnWithVal(ret, Some(optimize_expr(expr, state)), pos)
        }
        // All other statements - skip
        stmt => stmt,
    }
}

/// Optimize an expression.
fn optimize_expr(expr: Expr, state: &mut State) -> Expr {
    // These keywords are handled specially
    const DONT_EVAL_KEYWORDS: &[&str] = &[
        KEYWORD_PRINT,      // side effects
        KEYWORD_DEBUG,      // side effects
        KEYWORD_EVAL,       // arbitrary scripts
        KEYWORD_IS_DEF_FN,  // functions collection is volatile
        KEYWORD_IS_DEF_VAR, // variables scope is volatile
    ];

    match expr {
        // expr - do not promote because there is a reason it is wrapped in an `Expr::Expr`
        Expr::Expr(x) => Expr::Expr(Box::new(optimize_expr(*x, state))),
        // { stmt }
        Expr::Stmt(x) => match x.0 {
            // {} -> ()
            Stmt::Noop(_) => {
                state.set_dirty();
                Expr::Unit(x.1)
            }
            // { expr } -> expr
            Stmt::Expr(expr) => {
                state.set_dirty();
                optimize_expr(expr, state)
            }
            // { stmt }
            stmt => Expr::Stmt(Box::new((optimize_stmt(stmt, state, true), x.1))),
        },

        // lhs.rhs
        #[cfg(not(feature = "no_object"))]
        Expr::Dot(x) => match (x.lhs, x.rhs) {
            // map.string
            (Expr::Map(m), Expr::Property(p)) if m.0.iter().all(|(_, x)| x.is_pure()) => {
                let ((prop, _, _), _) = p.as_ref();
                // Map literal where everything is pure - promote the indexed item.
                // All other items can be thrown away.
                state.set_dirty();
                let pos = m.1;
                m.0.into_iter().find(|((name, _), _)| name == prop)
                    .map(|(_, mut expr)| { expr.set_position(pos); expr })
                    .unwrap_or_else(|| Expr::Unit(pos))
            }
            // lhs.rhs
            (lhs, rhs) => Expr::Dot(Box::new(BinaryExpr {
                lhs: optimize_expr(lhs, state),
                rhs: optimize_expr(rhs, state),
                pos: x.pos
            }))
        }

        // lhs[rhs]
        #[cfg(not(feature = "no_index"))]
        Expr::Index(x) => match (x.lhs, x.rhs) {
            // array[int]
            (Expr::Array(mut a), Expr::IntegerConstant(i))
                if i.0 >= 0 && (i.0 as usize) < a.0.len() && a.0.iter().all(Expr::is_pure) =>
            {
                // Array literal where everything is pure - promote the indexed item.
                // All other items can be thrown away.
                state.set_dirty();
                let mut expr = a.0.remove(i.0 as usize);
                expr.set_position(a.1);
                expr
            }
            // map[string]
            (Expr::Map(m), Expr::StringConstant(s)) if m.0.iter().all(|(_, x)| x.is_pure()) => {
                // Map literal where everything is pure - promote the indexed item.
                // All other items can be thrown away.
                state.set_dirty();
                let pos = m.1;
                m.0.into_iter().find(|((name, _), _)| *name == s.0)
                    .map(|(_, mut expr)| { expr.set_position(pos); expr })
                    .unwrap_or_else(|| Expr::Unit(pos))
            }
            // string[int]
            (Expr::StringConstant(s), Expr::IntegerConstant(i)) if i.0 >= 0 && (i.0 as usize) < s.0.chars().count() => {
                // String literal indexing - get the character
                state.set_dirty();
                Expr::CharConstant(Box::new((s.0.chars().nth(i.0 as usize).unwrap(), s.1)))
            }
            // lhs[rhs]
            (lhs, rhs) => Expr::Index(Box::new(BinaryExpr {
                lhs: optimize_expr(lhs, state),
                rhs: optimize_expr(rhs, state),
                pos: x.pos
            })),
        },
        // [ items .. ]
        #[cfg(not(feature = "no_index"))]
        Expr::Array(a) => Expr::Array(Box::new((a.0
                                .into_iter().map(|expr| optimize_expr(expr, state))
                                .collect(), a.1))),
        // [ items .. ]
        #[cfg(not(feature = "no_object"))]
        Expr::Map(m) => Expr::Map(Box::new((m.0
                            .into_iter().map(|((key, pos), expr)| ((key, pos), optimize_expr(expr, state)))
                            .collect(), m.1))),
        // lhs in rhs
        Expr::In(x) => match (x.lhs, x.rhs) {
            // "xxx" in "xxxxx"
            (Expr::StringConstant(a), Expr::StringConstant(b)) => {
                state.set_dirty();
                if b.0.contains(a.0.as_str()) { Expr::True(a.1) } else { Expr::False(a.1) }
            }
            // 'x' in "xxxxx"
            (Expr::CharConstant(a), Expr::StringConstant(b)) => {
                state.set_dirty();
                if b.0.contains(a.0) { Expr::True(a.1) } else { Expr::False(a.1) }
            }
            // "xxx" in #{...}
            (Expr::StringConstant(a), Expr::Map(b)) => {
                state.set_dirty();
                if b.0.iter().find(|((name, _), _)| *name == a.0).is_some() {
                    Expr::True(a.1)
                } else {
                    Expr::False(a.1)
                }
            }
            // 'x' in #{...}
            (Expr::CharConstant(a), Expr::Map(b)) => {
                state.set_dirty();
                let ch = a.0.to_string();

                if b.0.iter().find(|((name, _), _)| name == &ch).is_some() {
                    Expr::True(a.1)
                } else {
                    Expr::False(a.1)
                }
            }
            // lhs in rhs
            (lhs, rhs) => Expr::In(Box::new(BinaryExpr {
                lhs: optimize_expr(lhs, state),
                rhs: optimize_expr(rhs, state),
                pos: x.pos
            })),
        },
        // lhs && rhs
        Expr::And(x) => match (x.lhs, x.rhs) {
            // true && rhs -> rhs
            (Expr::True(_), rhs) => {
                state.set_dirty();
                rhs
            }
            // false && rhs -> false
            (Expr::False(pos), _) => {
                state.set_dirty();
                Expr::False(pos)
            }
            // lhs && true -> lhs
            (lhs, Expr::True(_)) => {
                state.set_dirty();
                optimize_expr(lhs, state)
            }
            // lhs && rhs
            (lhs, rhs) => Expr::And(Box::new(BinaryExpr {
                lhs: optimize_expr(lhs, state),
                rhs: optimize_expr(rhs, state),
                pos: x.pos
            })),
        },
        // lhs || rhs
        Expr::Or(x) => match (x.lhs, x.rhs) {
            // false || rhs -> rhs
            (Expr::False(_), rhs) => {
                state.set_dirty();
                rhs
            }
            // true || rhs -> true
            (Expr::True(pos), _) => {
                state.set_dirty();
                Expr::True(pos)
            }
            // lhs || false
            (lhs, Expr::False(_)) => {
                state.set_dirty();
                optimize_expr(lhs, state)
            }
            // lhs || rhs
            (lhs, rhs) => Expr::Or(Box::new(BinaryExpr {
                lhs: optimize_expr(lhs, state),
                rhs: optimize_expr(rhs, state),
                pos: x.pos
            })),
        },

        // Do not call some special keywords
        Expr::FnCall(mut x) if DONT_EVAL_KEYWORDS.contains(&(x.0).0.as_ref()) => {
            x.3 = x.3.into_iter().map(|a| optimize_expr(a, state)).collect();
            Expr::FnCall(x)
        }

        // Call built-in operators
        Expr::FnCall(mut x)
                if x.1.is_none() // Non-qualified
                && state.optimization_level == OptimizationLevel::Simple // simple optimizations
                && x.3.len() == 2 // binary call
                && x.3.iter().all(Expr::is_constant) // all arguments are constants
                && !is_valid_identifier((x.0).0.chars()) // cannot be scripted
        => {
            let ((name, _, _, pos), _, _, args, _) = x.as_mut();

            let arg_values: StaticVec<_> = args.iter().map(|e| e.get_constant_value().unwrap()).collect();
            let arg_types: StaticVec<_> = arg_values.iter().map(Dynamic::type_id).collect();

            // Search for overloaded operators (can override built-in).
            if !state.engine.has_override_by_name_and_arguments(state.lib, name, arg_types.as_ref(), false) {
                if let Some(expr) = run_builtin_binary_op(name, &arg_values[0], &arg_values[1])
                                        .ok().flatten()
                                        .and_then(|result| map_dynamic_to_expr(result, *pos))
                {
                    state.set_dirty();
                    return expr;
                }
            }

            x.3 = x.3.into_iter().map(|a| optimize_expr(a, state)).collect();
            Expr::FnCall(x)
        }

        // Eagerly call functions
        Expr::FnCall(mut x)
                if x.1.is_none() // Non-qualified
                && state.optimization_level == OptimizationLevel::Full // full optimizations
                && x.3.iter().all(Expr::is_constant) // all arguments are constants
        => {
            let ((name, _, _, pos), _, _, args, def_value) = x.as_mut();

            // First search for script-defined functions (can override built-in)
            #[cfg(not(feature = "no_function"))]
            let has_script_fn = state.lib.iter().any(|&m| m.get_script_fn(name, args.len(), false).is_some());
            #[cfg(feature = "no_function")]
            let has_script_fn = false;

            if !has_script_fn {
                let mut arg_values: StaticVec<_> = args.iter().map(|e| e.get_constant_value().unwrap()).collect();

                // Save the typename of the first argument if it is `type_of()`
                // This is to avoid `call_args` being passed into the closure
                let arg_for_type_of = if name == KEYWORD_TYPE_OF && arg_values.len() == 1 {
                    state.engine.map_type_name(arg_values[0].type_name())
                } else {
                    ""
                };

                if let Some(expr) = call_fn_with_constant_arguments(&state, name, arg_values.as_mut())
                                        .or_else(|| {
                                            if !arg_for_type_of.is_empty() {
                                                // Handle `type_of()`
                                                Some(arg_for_type_of.to_string().into())
                                            } else {
                                                // Otherwise use the default value, if any
                                                def_value.map(|v| v.into())
                                            }
                                        })
                                        .and_then(|result| map_dynamic_to_expr(result, *pos))
                {
                    state.set_dirty();
                    return expr;
                }
            }

            x.3 = x.3.into_iter().map(|a| optimize_expr(a, state)).collect();
            Expr::FnCall(x)
        }

        // id(args ..) -> optimize function call arguments
        Expr::FnCall(mut x) => {
            x.3 = x.3.into_iter().map(|a| optimize_expr(a, state)).collect();
            Expr::FnCall(x)
        }

        // constant-name
        Expr::Variable(x) if x.1.is_none() && state.contains_constant(&(x.0).0) => {
            let (name, pos) = x.0;
            state.set_dirty();

            // Replace constant with value
            let mut expr = state.find_constant(&name).unwrap().clone();
            expr.set_position(pos);
            expr
        }

        // Custom syntax
        #[cfg(feature = "internals")]
        Expr::Custom(x) => Expr::Custom(Box::new((
            CustomExpr(
                (x.0).0.into_iter().map(|expr| optimize_expr(expr, state)).collect(),
                (x.0).1),
            x.1
        ))),

        // All other expressions - skip
        expr => expr,
    }
}

fn optimize(
    statements: Vec<Stmt>,
    engine: &Engine,
    scope: &Scope,
    lib: &[&Module],
    level: OptimizationLevel,
) -> Vec<Stmt> {
    // If optimization level is None then skip optimizing
    if level == OptimizationLevel::None {
        return statements;
    }

    // Set up the state
    let mut state = State::new(engine, lib, level);

    // Add constants from the scope into the state
    scope
        .to_iter()
        // Get all the constants that can be made into a constant literal.
        .filter(|ScopeEntry { typ, .. }| typ.is_constant())
        .for_each(
            |ScopeEntry {
                 name, expr, value, ..
             }| {
                if let Some(val) = expr
                    .as_ref()
                    .map(|expr| expr.as_ref().clone())
                    .or_else(|| map_dynamic_to_expr(value.clone(), Position::none()))
                {
                    state.push_constant(name.as_ref(), val);
                }
            },
        );

    let orig_constants_len = state.constants.len();

    let mut result = statements;

    // Optimization loop
    loop {
        state.reset();
        state.restore_constants(orig_constants_len);

        let num_statements = result.len();

        result = result
            .into_iter()
            .enumerate()
            .map(|(i, stmt)| {
                match stmt {
                    Stmt::Const(var_def, Some(expr), pos) => {
                        // Load constants
                        let expr = optimize_expr(expr, &mut state);

                        if expr.is_literal() {
                            state.push_constant(&var_def.0, expr.clone());
                        }

                        // Keep it in the global scope
                        if expr.is_unit() {
                            state.set_dirty();
                            Stmt::Const(var_def, None, pos)
                        } else {
                            Stmt::Const(var_def, Some(expr), pos)
                        }
                    }
                    Stmt::Const(ref var_def, None, _) => {
                        state.push_constant(&var_def.0, Expr::Unit(var_def.1));

                        // Keep it in the global scope
                        stmt
                    }
                    _ => {
                        // Keep all variable declarations at this level
                        // and always keep the last return value
                        let keep = match stmt {
                            Stmt::Let(_, _, _) => true,
                            #[cfg(not(feature = "no_module"))]
                            Stmt::Import(_, _, _) => true,
                            _ => i == num_statements - 1,
                        };
                        optimize_stmt(stmt, &mut state, keep)
                    }
                }
            })
            .collect();

        if !state.is_dirty() {
            break;
        }
    }

    // Eliminate code that is pure but always keep the last statement
    let last_stmt = result.pop();

    // Remove all pure statements at global level
    result.retain(|stmt| !stmt.is_pure());

    // Add back the last statement unless it is a lone No-op
    if let Some(stmt) = last_stmt {
        if !result.is_empty() || !stmt.is_noop() {
            result.push(stmt);
        }
    }

    result
}

/// Optimize an AST.
pub fn optimize_into_ast(
    engine: &Engine,
    scope: &Scope,
    statements: Vec<Stmt>,
    _functions: Vec<ScriptFnDef>,
    level: OptimizationLevel,
) -> AST {
    let level = if cfg!(feature = "no_optimize") {
        OptimizationLevel::None
    } else {
        level
    };

    #[cfg(not(feature = "no_function"))]
    let lib = {
        let mut module = Module::new();

        if !level.is_none() {
            // We only need the script library's signatures for optimization purposes
            let mut lib2 = Module::new();

            _functions
                .iter()
                .map(|fn_def| {
                    ScriptFnDef {
                        name: fn_def.name.clone(),
                        access: fn_def.access,
                        body: Default::default(),
                        params: fn_def.params.clone(),
                        #[cfg(not(feature = "no_closure"))]
                        externals: fn_def.externals.clone(),
                        pos: fn_def.pos,
                        lib: None,
                    }
                    .into()
                })
                .for_each(|fn_def| {
                    lib2.set_script_fn(fn_def);
                });

            _functions
                .into_iter()
                .map(|mut fn_def| {
                    let pos = fn_def.body.position();

                    // Optimize the function body
                    let mut body =
                        optimize(vec![fn_def.body], engine, &Scope::new(), &[&lib2], level);

                    // {} -> Noop
                    fn_def.body = match body.pop().unwrap_or_else(|| Stmt::Noop(pos)) {
                        // { return val; } -> val
                        Stmt::ReturnWithVal((ReturnType::Return, _), Some(expr), _) => {
                            Stmt::Expr(expr)
                        }
                        // { return; } -> ()
                        Stmt::ReturnWithVal((ReturnType::Return, pos), None, _) => {
                            Stmt::Expr(Expr::Unit(pos))
                        }
                        // All others
                        stmt => stmt,
                    };
                    fn_def.into()
                })
                .for_each(|fn_def| {
                    module.set_script_fn(fn_def);
                });
        } else {
            _functions.into_iter().for_each(|fn_def| {
                module.set_script_fn(fn_def.into());
            });
        }

        module
    };

    #[cfg(feature = "no_function")]
    let lib = Default::default();

    AST::new(
        match level {
            OptimizationLevel::None => statements,
            OptimizationLevel::Simple | OptimizationLevel::Full => {
                optimize(statements, engine, &scope, &[&lib], level)
            }
        },
        lib,
    )
}
