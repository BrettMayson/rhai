#![cfg(not(feature = "no_module"))]
use rhai::{
    module_resolvers::StaticModuleResolver, Dynamic, Engine, EvalAltResult, ImmutableString,
    Module, ParseError, ParseErrorType, Scope, INT,
};

#[test]
fn test_module() {
    let mut module = Module::new();
    module.set_var("answer", 42 as INT);

    assert!(module.contains_var("answer"));
    assert_eq!(module.get_var_value::<INT>("answer").unwrap(), 42);
}

#[test]
fn test_module_sub_module() -> Result<(), Box<EvalAltResult>> {
    let mut module = Module::new();

    let mut sub_module = Module::new();

    let mut sub_module2 = Module::new();
    sub_module2.set_var("answer", 41 as INT);

    let hash_inc = sub_module2.set_fn_1("inc", |x: INT| Ok(x + 1));

    sub_module.set_sub_module("universe", sub_module2);
    module.set_sub_module("life", sub_module);
    module.set_var("MYSTIC_NUMBER", Dynamic::from(42 as INT));

    assert!(module.contains_sub_module("life"));
    let m = module.get_sub_module("life").unwrap();

    assert!(m.contains_sub_module("universe"));
    let m2 = m.get_sub_module("universe").unwrap();

    assert!(m2.contains_var("answer"));
    assert!(m2.contains_fn(hash_inc, false));

    assert_eq!(m2.get_var_value::<INT>("answer").unwrap(), 41);

    let mut resolver = StaticModuleResolver::new();
    resolver.insert("question", module);

    let mut engine = Engine::new();
    engine.set_module_resolver(Some(resolver));

    assert_eq!(
        engine.eval::<INT>(r#"import "question" as q; q::MYSTIC_NUMBER"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "question" as q; q::life::universe::answer + 1"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(
            r#"import "question" as q; q::life::universe::inc(q::life::universe::answer)"#
        )?,
        42
    );

    Ok(())
}

#[test]
fn test_module_resolver() -> Result<(), Box<EvalAltResult>> {
    let mut resolver = StaticModuleResolver::new();

    let mut module = Module::new();

    module.set_var("answer", 42 as INT);
    module.set_fn_4("sum".to_string(), |x: INT, y: INT, z: INT, w: INT| {
        Ok(x + y + z + w)
    });
    module.set_fn_1_mut("double".to_string(), |x: &mut INT| {
        *x *= 2;
        Ok(())
    });

    #[cfg(not(feature = "no_float"))]
    module.set_fn_4_mut(
        "sum_of_three_args".to_string(),
        |target: &mut INT, a: INT, b: INT, c: f64| {
            *target = a + b + c as INT;
            Ok(())
        },
    );

    resolver.insert("hello", module);

    let mut engine = Engine::new();
    engine.set_module_resolver(Some(resolver));

    assert_eq!(
        engine.eval::<INT>(
            r#"
                import "hello" as h1;
                import "hello" as h2;
                h1::sum(h2::answer, -10, 3, 7)
            "#
        )?,
        42
    );

    assert_eq!(
        engine.eval::<INT>(
            r#"
                import "hello" as h1;
                import "hello" as h2;
                let x = 42;
                h1::sum(x, -10, 3, 7)
            "#
        )?,
        42
    );

    assert_eq!(
        engine.eval::<INT>(
            r#"
                import "hello" as h1;
                import "hello" as h2;
                let x = 42;
                h1::sum(x, 0, 0, 0);
                x
            "#
        )?,
        42
    );

    assert_eq!(
        engine.eval::<INT>(
            r#"
                import "hello" as h;
                let x = 21;
                h::double(x);
                x
            "#
        )?,
        42
    );
    #[cfg(not(feature = "no_float"))]
    {
        assert_eq!(
            engine.eval::<INT>(
                r#"
                        import "hello" as h;
                        let x = 21;
                        h::sum_of_three_args(x, 14, 26, 2.0);
                        x
                    "#
            )?,
            42
        );
    }

    #[cfg(not(feature = "unchecked"))]
    {
        engine.set_max_modules(5);

        assert!(matches!(
            *engine
                .eval::<INT>(
                    r#"
                        let sum = 0;

                        for x in range(0, 10) {
                            import "hello" as h;
                            sum += h::answer;
                        }

                        sum
                    "#
                )
                .expect_err("should error"),
            EvalAltResult::ErrorTooManyModules(_)
        ));

        #[cfg(not(feature = "no_function"))]
        assert!(matches!(
            *engine
                .eval::<INT>(
                    r#"
                        let sum = 0;

                        fn foo() {
                            import "hello" as h;
                            sum += h::answer;
                        }

                        for x in range(0, 10) {
                            foo();
                        }

                        sum
                    "#
                )
                .expect_err("should error"),
            EvalAltResult::ErrorInFunctionCall(fn_name, _, _) if fn_name == "foo"
        ));

        engine.set_max_modules(1000);

        #[cfg(not(feature = "no_function"))]
        engine.eval::<()>(
            r#"
                fn foo() {
                    import "hello" as h;
                }

                for x in range(0, 10) {
                    foo();
                }
            "#,
        )?;
    }

    Ok(())
}

#[test]
#[cfg(not(feature = "no_function"))]
fn test_module_from_ast() -> Result<(), Box<EvalAltResult>> {
    let mut engine = Engine::new();

    let mut resolver1 = StaticModuleResolver::new();
    let mut sub_module = Module::new();
    sub_module.set_var("foo", true);
    resolver1.insert("another module", sub_module);

    let ast = engine.compile(
        r#"
            // Functions become module functions
            fn calc(x) {
                x + 1
            }
            fn add_len(x, y) {
                x + len(y)
            }
            fn cross_call(x) {
                calc(x)
            }
            private fn hidden() {
                throw "you shouldn't see me!";
            }
        
            // Imported modules become sub-modules
            import "another module" as extra;
        
            // Variables defined at global level become module variables
            const x = 123;
            let foo = 41;
            let hello;
        
            // Final variable values become constant module variable values
            foo = calc(foo);
            hello = "hello, " + foo + " worlds!";

            export
                x as abc,
                foo,
                hello;
        "#,
    )?;

    engine.set_module_resolver(Some(resolver1));

    let module = Module::eval_ast_as_new(Scope::new(), &ast, &engine)?;

    let mut resolver2 = StaticModuleResolver::new();
    resolver2.insert("testing", module);
    engine.set_module_resolver(Some(resolver2));

    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as ttt; ttt::abc"#)?,
        123
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as ttt; ttt::foo"#)?,
        42
    );
    assert!(engine.eval::<bool>(r#"import "testing" as ttt; ttt::extra::foo"#)?);
    assert_eq!(
        engine.eval::<String>(r#"import "testing" as ttt; ttt::hello"#)?,
        "hello, 42 worlds!"
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as ttt; ttt::calc(999)"#)?,
        1000
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as ttt; ttt::cross_call(999)"#)?,
        1000
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as ttt; ttt::add_len(ttt::foo, ttt::hello)"#)?,
        59
    );
    assert!(matches!(
        *engine
            .consume(r#"import "testing" as ttt; ttt::hidden()"#)
            .expect_err("should error"),
        EvalAltResult::ErrorFunctionNotFound(fn_name, _) if fn_name == "ttt::hidden ()"
    ));

    Ok(())
}

#[test]
fn test_module_export() -> Result<(), Box<EvalAltResult>> {
    let engine = Engine::new();

    assert!(matches!(
        engine.compile(r"let x = 10; { export x; }").expect_err("should error"),
        ParseError(x, _) if *x == ParseErrorType::WrongExport
    ));

    #[cfg(not(feature = "no_function"))]
    assert!(matches!(
        engine.compile(r"fn abc(x) { export x; }").expect_err("should error"),
        ParseError(x, _) if *x == ParseErrorType::WrongExport
    ));

    Ok(())
}

#[test]
fn test_module_str() -> Result<(), Box<EvalAltResult>> {
    fn test_fn(_input: ImmutableString) -> Result<INT, Box<EvalAltResult>> {
        Ok(42)
    }
    fn test_fn2(_input: &str) -> Result<INT, Box<EvalAltResult>> {
        Ok(42)
    }
    fn test_fn3(_input: String) -> Result<INT, Box<EvalAltResult>> {
        Ok(42)
    }

    let mut engine = rhai::Engine::new();
    let mut module = Module::new();
    module.set_fn_1("test", test_fn);
    module.set_fn_1("test2", test_fn2);
    module.set_fn_1("test3", test_fn3);

    let mut static_modules = rhai::module_resolvers::StaticModuleResolver::new();
    static_modules.insert("test", module);
    engine.set_module_resolver(Some(static_modules));

    assert_eq!(
        engine.eval::<INT>(r#"import "test" as test; test::test("test");"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "test" as test; test::test2("test");"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "test" as test; test::test3("test");"#)?,
        42
    );

    Ok(())
}

#[cfg(not(feature = "no_function"))]
#[test]
fn test_module_ast_namespace() -> Result<(), Box<EvalAltResult>> {
    let script = r#"
        fn foo(x) { x + 1 }
        fn bar(x) { foo(x) }
    "#;

    let mut engine = Engine::new();

    let ast = engine.compile(script)?;

    let module = Module::eval_ast_as_new(Default::default(), &ast, &engine)?;

    let mut resolver = StaticModuleResolver::new();
    resolver.insert("testing", module);
    engine.set_module_resolver(Some(resolver));

    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as t; t::foo(41)"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"import "testing" as t; t::bar(41)"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"fn foo(x) { x - 1 } import "testing" as t; t::foo(41)"#)?,
        42
    );
    assert_eq!(
        engine.eval::<INT>(r#"fn foo(x) { x - 1 } import "testing" as t; t::bar(41)"#)?,
        42
    );

    Ok(())
}

#[cfg(not(feature = "no_function"))]
#[test]
fn test_module_ast_namespace2() -> Result<(), Box<EvalAltResult>> {
    use rhai::{Engine, Module, Scope};

    const MODULE_TEXT: &str = r#"
        fn run_function(function) {
            call(function)
        }
    "#;

    const SCRIPT: &str = r#"
        import "test_module" as test;

        fn foo() {
            print("foo");
        }

        test::run_function(Fn("foo"));
    "#;

    let mut engine = Engine::new();
    let module_ast = engine.compile(MODULE_TEXT)?;
    let module = Module::eval_ast_as_new(Scope::new(), &module_ast, &engine)?;
    let mut static_modules = rhai::module_resolvers::StaticModuleResolver::new();
    static_modules.insert("test_module", module);
    engine.set_module_resolver(Some(static_modules));

    engine.consume(SCRIPT)?;

    Ok(())
}
