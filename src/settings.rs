//! Configuration settings for `Engine`.

use crate::engine::Engine;
use crate::packages::PackageLibrary;
use crate::token::{is_valid_identifier, Token};

#[cfg(not(feature = "no_module"))]
use crate::module::ModuleResolver;

#[cfg(not(feature = "no_optimize"))]
use crate::optimize::OptimizationLevel;

use crate::stdlib::{format, string::String};

#[cfg(not(feature = "no_module"))]
use crate::stdlib::boxed::Box;

impl Engine {
    /// Load a new package into the `Engine`.
    /// Anything that can be converted into a `PackageLibrary` is accepted, including a simple `Module`.
    ///
    /// When searching for functions, packages loaded later are preferred.
    /// In other words, loaded packages are searched in reverse order.
    #[inline(always)]
    pub fn load_package(&mut self, package: impl Into<PackageLibrary>) -> &mut Self {
        // Push the package to the top - packages are searched in reverse order
        self.packages.push(package.into());
        self
    }

    /// Control whether and how the `Engine` will optimize an AST after compilation.
    ///
    /// Not available under the `no_optimize` feature.
    #[cfg(not(feature = "no_optimize"))]
    #[inline(always)]
    pub fn set_optimization_level(&mut self, optimization_level: OptimizationLevel) -> &mut Self {
        self.optimization_level = optimization_level;
        self
    }

    /// The current optimization level.
    /// It controls whether and how the `Engine` will optimize an AST after compilation.
    ///
    /// Not available under the `no_optimize` feature.
    #[cfg(not(feature = "no_optimize"))]
    #[inline(always)]
    pub fn optimization_level(&self) -> OptimizationLevel {
        self.optimization_level
    }

    /// Set the maximum levels of function calls allowed for a script in order to avoid
    /// infinite recursion and stack overflows.
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn set_max_call_levels(&mut self, levels: usize) -> &mut Self {
        self.limits_set.max_call_stack_depth = levels;
        self
    }

    /// The maximum levels of function calls allowed for a script.
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn max_call_levels(&self) -> usize {
        self.limits_set.max_call_stack_depth
    }

    /// Set the maximum number of operations allowed for a script to run to avoid
    /// consuming too much resources (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn set_max_operations(&mut self, operations: u64) -> &mut Self {
        self.limits_set.max_operations = if operations == u64::MAX {
            0
        } else {
            operations
        };
        self
    }

    /// The maximum number of operations allowed for a script to run (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn max_operations(&self) -> u64 {
        self.limits_set.max_operations
    }

    /// Set the maximum number of imported modules allowed for a script.
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_module"))]
    #[inline(always)]
    pub fn set_max_modules(&mut self, modules: usize) -> &mut Self {
        self.limits_set.max_modules = modules;
        self
    }

    /// The maximum number of imported modules allowed for a script.
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_module"))]
    #[inline(always)]
    pub fn max_modules(&self) -> usize {
        self.limits_set.max_modules
    }

    /// Set the depth limits for expressions (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn set_max_expr_depths(
        &mut self,
        max_expr_depth: usize,
        #[cfg(not(feature = "no_function"))] max_function_expr_depth: usize,
    ) -> &mut Self {
        self.limits_set.max_expr_depth = if max_expr_depth == usize::MAX {
            0
        } else {
            max_expr_depth
        };
        #[cfg(not(feature = "no_function"))]
        {
            self.limits_set.max_function_expr_depth = if max_function_expr_depth == usize::MAX {
                0
            } else {
                max_function_expr_depth
            };
        }
        self
    }

    /// The depth limit for expressions (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn max_expr_depth(&self) -> usize {
        self.limits_set.max_expr_depth
    }

    /// The depth limit for expressions in functions (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_function"))]
    #[inline(always)]
    pub fn max_function_expr_depth(&self) -> usize {
        self.limits_set.max_function_expr_depth
    }

    /// Set the maximum length of strings (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn set_max_string_size(&mut self, max_size: usize) -> &mut Self {
        self.limits_set.max_string_size = if max_size == usize::MAX { 0 } else { max_size };
        self
    }

    /// The maximum length of strings (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[inline(always)]
    pub fn max_string_size(&self) -> usize {
        self.limits_set.max_string_size
    }

    /// Set the maximum length of arrays (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_index"))]
    #[inline(always)]
    pub fn set_max_array_size(&mut self, max_size: usize) -> &mut Self {
        self.limits_set.max_array_size = if max_size == usize::MAX { 0 } else { max_size };
        self
    }

    /// The maximum length of arrays (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_index"))]
    #[inline(always)]
    pub fn max_array_size(&self) -> usize {
        self.limits_set.max_array_size
    }

    /// Set the maximum length of object maps (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_object"))]
    #[inline(always)]
    pub fn set_max_map_size(&mut self, max_size: usize) -> &mut Self {
        self.limits_set.max_map_size = if max_size == usize::MAX { 0 } else { max_size };
        self
    }

    /// The maximum length of object maps (0 for unlimited).
    #[cfg(not(feature = "unchecked"))]
    #[cfg(not(feature = "no_object"))]
    #[inline(always)]
    pub fn max_map_size(&self) -> usize {
        self.limits_set.max_map_size
    }

    /// Set the module resolution service used by the `Engine`.
    ///
    /// Not available under the `no_module` feature.
    #[cfg(not(feature = "no_module"))]
    #[inline(always)]
    pub fn set_module_resolver(
        &mut self,
        resolver: Option<impl ModuleResolver + 'static>,
    ) -> &mut Self {
        self.module_resolver = resolver.map(|f| Box::new(f) as Box<dyn ModuleResolver>);
        self
    }

    /// Disable a particular keyword or operator in the language.
    ///
    /// # Examples
    ///
    /// The following will raise an error during parsing because the `if` keyword is disabled
    /// and is recognized as a variable name!
    ///
    /// ```rust,should_panic
    /// # fn main() -> Result<(), rhai::ParseError> {
    /// use rhai::Engine;
    ///
    /// let mut engine = Engine::new();
    ///
    /// engine.disable_symbol("if");    // disable the 'if' keyword
    ///
    /// engine.compile("let x = if true { 42 } else { 0 };")?;
    /// //                      ^ 'if' is rejected as a reserved keyword
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The following will raise an error during parsing because the `+=` operator is disabled.
    ///
    /// ```rust,should_panic
    /// # fn main() -> Result<(), rhai::ParseError> {
    /// use rhai::Engine;
    ///
    /// let mut engine = Engine::new();
    ///
    /// engine.disable_symbol("+=");    // disable the '+=' operator
    ///
    /// engine.compile("let x = 42; x += 1;")?;
    /// //                            ^ unknown operator
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn disable_symbol(&mut self, symbol: &str) -> &mut Self {
        self.disabled_symbols.insert(symbol.into());
        self
    }

    /// Register a custom operator into the language.
    ///
    /// The operator must be a valid identifier (i.e. it cannot be a symbol).
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn main() -> Result<(), Box<rhai::EvalAltResult>> {
    /// use rhai::{Engine, RegisterFn};
    ///
    /// let mut engine = Engine::new();
    ///
    /// // Register a custom operator called 'foo' and give it
    /// // a precedence of 160 (i.e. between +|- and *|/).
    /// engine.register_custom_operator("foo", 160).unwrap();
    ///
    /// // Register a binary function named 'foo'
    /// engine.register_fn("foo", |x: i64, y: i64| (x * y) - (x + y));
    ///
    /// assert_eq!(
    ///     engine.eval_expression::<i64>("1 + 2 * 3 foo 4 - 5 / 6")?,
    ///     15
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_custom_operator(
        &mut self,
        keyword: &str,
        precedence: u8,
    ) -> Result<&mut Self, String> {
        if !is_valid_identifier(keyword.chars()) {
            return Err(format!("not a valid identifier: '{}'", keyword).into());
        }

        match Token::lookup_from_syntax(keyword) {
            // Standard identifiers, reserved keywords and custom keywords are OK
            None | Some(Token::Reserved(_)) | Some(Token::Custom(_)) => (),
            // Disabled keywords are also OK
            Some(token) if !self.disabled_symbols.contains(token.syntax().as_ref()) => (),
            // Active standard keywords cannot be made custom
            Some(_) => return Err(format!("'{}' is a reserved keyword", keyword).into()),
        }

        // Add to custom keywords
        self.custom_keywords
            .insert(keyword.into(), Some(precedence));

        Ok(self)
    }
}
