use std::marker::PhantomData;

use liquid::{model::KString, ParserBuilder, ValueView};
use liquid_core::{Display_filter, Filter, FilterParameters, ParseFilter, Value};
use liquid_derive::FilterReflection;
use pulldown_cmark::{html, Options, Parser};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params_from_iter;
use serde_json::Map;
use serde_rusqlite::from_rows;

#[derive(Clone, FilterReflection)]
#[filter(
    name = "query",
    description = "Run an SQL query",
    parameters(QueryArgs),
    parsed(QueryFilter)
)]
pub struct Query {
    db: Pool<SqliteConnectionManager>,
}

impl ParseFilter for Query {
    fn parse(
        &self,
        arguments: liquid_core::parser::FilterArguments,
    ) -> liquid_core::Result<Box<dyn Filter>> {
        let params = QueryArgs::from_args(arguments)?;

        let db = self.db.clone();

        Ok(Box::new(QueryFilter { params, db }))
    }

    fn reflection(&self) -> &dyn liquid_core::FilterReflection {
        self as &dyn liquid_core::FilterReflection
    }
}

#[derive(Debug, FilterParameters)]
struct QueryArgs {
    #[parameter(description = "Parameters for the query")]
    params: Option<liquid_core::Expression>,
}

#[derive(Debug, Display_filter)]
#[name = "query"]
struct QueryFilter {
    #[parameters]
    params: QueryArgs,
    db: Pool<SqliteConnectionManager>,
}

impl Filter for QueryFilter {
    fn evaluate(
        &self,
        input: &dyn liquid::ValueView,
        runtime: &dyn liquid_core::Runtime,
    ) -> liquid_core::Result<liquid_core::Value> {
        let args: Vec<serde_json::Value> = if let Some(params) = &self.params.params {
            let args = params.evaluate(runtime)?;
            let params_raw = args.as_view().to_value();
            let args_str = serde_json::to_string(&params_raw).unwrap();
            serde_json::from_str(&args_str).unwrap()
        } else {
            vec![]
        };

        let input = input
            .as_scalar()
            .ok_or_else(|| invalid_input("String expected"))?;

        let s = input.to_kstr();
        let sql = s.as_str();
        let conn = self
            .db
            .get()
            .map_err(|_| liquid::Error::with_msg("Couldn't get db"))?;
        let mut stmt = conn
            .prepare(sql)
            .map_err(|_| liquid::Error::with_msg("Could not prepare statment"))?;
        let rows = stmt.query(params_from_iter(args)).unwrap();
        let r = from_rows::<Map<String, serde_json::Value>>(rows);
        let mut r2 = vec![];
        for row in r {
            let row = row.unwrap();
            r2.push(serde_json::Value::Object(row))
        }
        log::trace!("{:?}", r2);
        let r3 = serde_json::Value::Array(r2);

        let r4: liquid_core::Value = serde_json::from_value(r3).unwrap();

        Ok(r4)
    }
}

fn invalid_input<S>(cause: S) -> liquid::Error
where
    S: Into<KString>,
{
    liquid::Error::with_msg("Invalid input").context("cause", cause)
}

pub struct FilterSum<M, F>(PhantomData<(M, F)>);

pub trait Filterable: Sized {
    type ConstructArgs;
    fn new(args: Self::ConstructArgs) -> Self;
    fn register(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder;
}

pub trait FilterTerm {
    type ConstructArgs;
    fn expand(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder;
}

impl<T> FilterTerm for T
where
    T: ParseFilter + Filterable,
{
    type ConstructArgs = T::ConstructArgs;
    fn expand(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder {
        T::register(builder, args)
    }
}

impl<M, F> FilterTerm for FilterSum<M, F>
where
    M: Filterable,
    F: FilterTerm,
{
    type ConstructArgs = (M::ConstructArgs, F::ConstructArgs);
    fn expand(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder {
        F::expand(M::register(builder, args.0), args.1)
    }
}

impl<M, F> Filterable for FilterSum<M, F>
where
    M: Filterable,
    F: FilterTerm,
{
    type ConstructArgs = <Self as FilterTerm>::ConstructArgs;
    fn new(_args: Self::ConstructArgs) -> Self {
        Self(PhantomData)
    }
    fn register(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder {
        Self::expand(builder, args)
    }
}

impl Filterable for Query {
    type ConstructArgs = Pool<SqliteConnectionManager>;
    fn new(args: Self::ConstructArgs) -> Self {
        Self { db: args }
    }

    fn register(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder {
        builder.filter(Self::new(args))
    }
}

#[derive(Clone, FilterReflection, ParseFilter)]
#[filter(
    name = "markdown",
    description = "Parse markdown into HTML",
    parsed(MarkdownFilter)
)]
pub struct Markdown;

#[derive(Debug, Display_filter, Default)]
#[name = "markdown"]
struct MarkdownFilter;

impl Filter for MarkdownFilter {
    fn evaluate(
        &self,
        input: &dyn ValueView,
        _runtime: &dyn liquid_core::Runtime,
    ) -> liquid_core::Result<liquid_core::Value> {
        let input_value = input
            .as_scalar()
            .ok_or_else(|| invalid_input("String expected"))?;
        let input = input_value.to_kstr();
        let source = input.as_str();

        let options = Options::all();
        let parser = Parser::new_ext(source, options);
        let mut html = String::new();
        html::push_html(&mut html, parser);

        Ok(Value::scalar(html))
    }
}

impl Filterable for Markdown {
    type ConstructArgs = ();

    fn new(_args: Self::ConstructArgs) -> Self {
        Self
    }

    fn register(builder: ParserBuilder, args: Self::ConstructArgs) -> ParserBuilder {
        builder.filter(Self::new(args))
    }
}
