//! The parser of JMESPath expressions, ported from `jmespath.parser` of
//! Python, which is a Pratt parser.

use super::Error;
use super::lexer::{Kind, Token, tokenize};
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone)]
pub enum Node {
    Literal(Value),
    Field(String),
    Identity,
    Current,
    Subexpression(Vec<Node>),
    IndexExpression(Vec<Node>),
    Index(i64),
    Slice([Option<i64>; 3]),
    Projection(Box<Node>, Box<Node>),
    ValueProjection(Box<Node>, Box<Node>),
    FilterProjection(Box<Node>, Box<Node>, Box<Node>),
    Flatten(Box<Node>),
    Not(Box<Node>),
    Pipe(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    And(Box<Node>, Box<Node>),
    Comparator(Comparator, Box<Node>, Box<Node>),
    Function(String, Vec<Node>),
    Expref(Box<Node>),
    MultiSelectList(Vec<Node>),
    MultiSelectHash(Vec<(String, Node)>),
}

const PROJECTION_STOP: u8 = 10;

fn binding_power(kind: Kind) -> u8 {
    match kind {
        Kind::Pipe => 1,
        Kind::Or => 2,
        Kind::And => 3,
        Kind::Eq | Kind::Gt | Kind::Lt | Kind::Gte | Kind::Lte | Kind::Ne => 5,
        Kind::Flatten => 9,
        Kind::Star => 20,
        Kind::Filter => 21,
        Kind::Dot => 40,
        Kind::Not => 45,
        Kind::LBrace => 50,
        Kind::LBracket => 55,
        Kind::LParen => 60,
        _ => 0,
    }
}

pub fn parse(expression: &str) -> Result<Node, Error> {
    let mut parser = Parser {
        tokens: tokenize(expression)?,
        index: 0,
        expression,
    };
    let node = parser.expression(0)?;
    if parser.current() != Kind::Eof {
        let token = parser.lookahead_token(0).clone();
        let message = format!("Unexpected token: {}", token.text());
        return Err(parser.parse_error(&token, message));
    }
    Ok(node)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    index: usize,
    expression: &'a str,
}

impl Parser<'_> {
    fn expression(&mut self, binding: u8) -> Result<Node, Error> {
        let token = self.lookahead_token(0).clone();
        self.advance();
        let mut left = self.nud(token)?;
        while binding < binding_power(self.current()) {
            let token = self.lookahead_token(0).clone();
            left = match token.kind {
                Kind::Dot
                | Kind::Pipe
                | Kind::Or
                | Kind::And
                | Kind::LParen
                | Kind::Filter
                | Kind::Eq
                | Kind::Ne
                | Kind::Gt
                | Kind::Gte
                | Kind::Lt
                | Kind::Lte
                | Kind::Flatten
                | Kind::LBracket => {
                    self.advance();
                    self.led(token.kind, left)?
                }
                _ => return Err(self.parse_error(&token, "invalid token".to_owned())),
            };
        }
        Ok(left)
    }

    fn nud(&mut self, token: Token) -> Result<Node, Error> {
        match token.kind {
            Kind::Literal => Ok(Node::Literal(token.value)),
            Kind::UnquotedIdentifier => Ok(Node::Field(token.text())),
            Kind::QuotedIdentifier => {
                if self.current() == Kind::LParen {
                    let mut token = self.lookahead_token(0).clone();
                    token.start = 0;
                    return Err(self.parse_error(
                        &token,
                        "Quoted identifier not allowed for function names.".to_owned(),
                    ));
                }
                Ok(Node::Field(token.text()))
            }
            Kind::Star => {
                let right = if self.current() == Kind::RBracket {
                    Node::Identity
                } else {
                    self.projection_rhs(binding_power(Kind::Star))?
                };
                Ok(Node::ValueProjection(
                    Box::new(Node::Identity),
                    Box::new(right),
                ))
            }
            Kind::Filter => self.led_filter(Node::Identity),
            Kind::LBrace => self.multi_select_hash(),
            Kind::LParen => {
                let expression = self.expression(0)?;
                self.expect(Kind::RParen)?;
                Ok(expression)
            }
            Kind::Flatten => {
                let left = Node::Flatten(Box::new(Node::Identity));
                let right = self.projection_rhs(binding_power(Kind::Flatten))?;
                Ok(Node::Projection(Box::new(left), Box::new(right)))
            }
            Kind::Not => {
                let expression = self.expression(binding_power(Kind::Not))?;
                Ok(Node::Not(Box::new(expression)))
            }
            Kind::LBracket => {
                if matches!(self.current(), Kind::Number | Kind::Colon) {
                    let right = self.index_expression()?;
                    self.project_if_slice(Node::Identity, right)
                } else if self.current() == Kind::Star && self.lookahead(1) == Kind::RBracket {
                    self.advance();
                    self.advance();
                    let right = self.projection_rhs(binding_power(Kind::Star))?;
                    Ok(Node::Projection(Box::new(Node::Identity), Box::new(right)))
                } else {
                    self.multi_select_list()
                }
            }
            Kind::Current => Ok(Node::Current),
            Kind::Expref => {
                let expression = self.expression(binding_power(Kind::Expref))?;
                Ok(Node::Expref(Box::new(expression)))
            }
            Kind::Eof => Err(self.incomplete()),
            _ => Err(self.parse_error(&token, "invalid token".to_owned())),
        }
    }

    fn led(&mut self, kind: Kind, left: Node) -> Result<Node, Error> {
        match kind {
            Kind::Dot => {
                if self.current() != Kind::Star {
                    let right = self.dot_rhs(binding_power(Kind::Dot))?;
                    match left {
                        Node::Subexpression(mut children) => {
                            children.push(right);
                            Ok(Node::Subexpression(children))
                        }
                        left => Ok(Node::Subexpression(vec![left, right])),
                    }
                } else {
                    self.advance();
                    let right = self.projection_rhs(binding_power(Kind::Dot))?;
                    Ok(Node::ValueProjection(Box::new(left), Box::new(right)))
                }
            }
            Kind::Pipe => {
                let right = self.expression(binding_power(Kind::Pipe))?;
                Ok(Node::Pipe(Box::new(left), Box::new(right)))
            }
            Kind::Or => {
                let right = self.expression(binding_power(Kind::Or))?;
                Ok(Node::Or(Box::new(left), Box::new(right)))
            }
            Kind::And => {
                let right = self.expression(binding_power(Kind::And))?;
                Ok(Node::And(Box::new(left), Box::new(right)))
            }
            Kind::LParen => {
                let Node::Field(name) = left else {
                    let token = self.lookahead_token(-2).clone();
                    let message = format!("Invalid function name '{}'", token.text());
                    return Err(self.parse_error(&token, message));
                };
                let mut args = Vec::new();
                while self.current() != Kind::RParen {
                    let expression = self.expression(0)?;
                    if self.current() == Kind::Comma {
                        self.expect(Kind::Comma)?;
                    }
                    args.push(expression);
                }
                self.expect(Kind::RParen)?;
                Ok(Node::Function(name, args))
            }
            Kind::Filter => self.led_filter(left),
            Kind::Eq => self.comparator(left, Comparator::Eq, kind),
            Kind::Ne => self.comparator(left, Comparator::Ne, kind),
            Kind::Gt => self.comparator(left, Comparator::Gt, kind),
            Kind::Gte => self.comparator(left, Comparator::Gte, kind),
            Kind::Lt => self.comparator(left, Comparator::Lt, kind),
            Kind::Lte => self.comparator(left, Comparator::Lte, kind),
            Kind::Flatten => {
                let left = Node::Flatten(Box::new(left));
                let right = self.projection_rhs(binding_power(Kind::Flatten))?;
                Ok(Node::Projection(Box::new(left), Box::new(right)))
            }
            Kind::LBracket => {
                if matches!(self.current(), Kind::Number | Kind::Colon) {
                    let right = self.index_expression()?;
                    match left {
                        Node::IndexExpression(mut children) => {
                            children.push(right);
                            Ok(Node::IndexExpression(children))
                        }
                        left => self.project_if_slice(left, right),
                    }
                } else {
                    self.expect(Kind::Star)?;
                    self.expect(Kind::RBracket)?;
                    let right = self.projection_rhs(binding_power(Kind::Star))?;
                    Ok(Node::Projection(Box::new(left), Box::new(right)))
                }
            }
            _ => unreachable!("led of {kind:?}"),
        }
    }

    fn led_filter(&mut self, left: Node) -> Result<Node, Error> {
        let condition = self.expression(0)?;
        self.expect(Kind::RBracket)?;
        let right = if self.current() == Kind::Flatten {
            Node::Identity
        } else {
            self.projection_rhs(binding_power(Kind::Filter))?
        };
        Ok(Node::FilterProjection(
            Box::new(left),
            Box::new(right),
            Box::new(condition),
        ))
    }

    fn comparator(
        &mut self,
        left: Node,
        comparator: Comparator,
        kind: Kind,
    ) -> Result<Node, Error> {
        let right = self.expression(binding_power(kind))?;
        Ok(Node::Comparator(
            comparator,
            Box::new(left),
            Box::new(right),
        ))
    }

    fn index_expression(&mut self) -> Result<Node, Error> {
        if self.lookahead(0) == Kind::Colon || self.lookahead(1) == Kind::Colon {
            return self.slice_expression();
        }
        let node = Node::Index(number(self.lookahead_token(0)));
        self.advance();
        self.expect(Kind::RBracket)?;
        Ok(node)
    }

    fn slice_expression(&mut self) -> Result<Node, Error> {
        let mut parts = [None; 3];
        let mut index = 0;
        while self.current() != Kind::RBracket && index < 3 {
            match self.current() {
                Kind::Colon => {
                    index += 1;
                    if index == 3 {
                        let token = self.lookahead_token(0).clone();
                        return Err(self.parse_error(&token, "syntax error".to_owned()));
                    }
                    self.advance();
                }
                Kind::Number => {
                    parts[index] = Some(number(self.lookahead_token(0)));
                    self.advance();
                }
                _ => {
                    let token = self.lookahead_token(0).clone();
                    return Err(self.parse_error(&token, "syntax error".to_owned()));
                }
            }
        }
        self.expect(Kind::RBracket)?;
        Ok(Node::Slice(parts))
    }

    fn project_if_slice(&mut self, left: Node, right: Node) -> Result<Node, Error> {
        let is_slice = matches!(right, Node::Slice(_));
        let index = Node::IndexExpression(vec![left, right]);
        if is_slice {
            let right = self.projection_rhs(binding_power(Kind::Star))?;
            Ok(Node::Projection(Box::new(index), Box::new(right)))
        } else {
            Ok(index)
        }
    }

    fn multi_select_list(&mut self) -> Result<Node, Error> {
        let mut expressions = Vec::new();
        loop {
            expressions.push(self.expression(0)?);
            if self.current() == Kind::RBracket {
                break;
            }
            self.expect(Kind::Comma)?;
        }
        self.expect(Kind::RBracket)?;
        Ok(Node::MultiSelectList(expressions))
    }

    fn multi_select_hash(&mut self) -> Result<Node, Error> {
        let mut pairs = Vec::new();
        loop {
            let key = self.lookahead_token(0).clone();
            if !matches!(
                self.current(),
                Kind::QuotedIdentifier | Kind::UnquotedIdentifier
            ) {
                return Err(self.expecting("['quoted_identifier', 'unquoted_identifier']", &key));
            }
            self.advance();
            self.expect(Kind::Colon)?;
            let value = self.expression(0)?;
            pairs.push((key.text(), value));
            if self.current() == Kind::Comma {
                self.expect(Kind::Comma)?;
            } else if self.current() == Kind::RBrace {
                self.expect(Kind::RBrace)?;
                break;
            }
        }
        Ok(Node::MultiSelectHash(pairs))
    }

    fn projection_rhs(&mut self, binding: u8) -> Result<Node, Error> {
        let current = self.current();
        if binding_power(current) < PROJECTION_STOP {
            Ok(Node::Identity)
        } else if matches!(current, Kind::LBracket | Kind::Filter) {
            self.expression(binding)
        } else if current == Kind::Dot {
            self.expect(Kind::Dot)?;
            self.dot_rhs(binding)
        } else {
            let token = self.lookahead_token(0).clone();
            Err(self.parse_error(&token, "syntax error".to_owned()))
        }
    }

    fn dot_rhs(&mut self, binding: u8) -> Result<Node, Error> {
        match self.current() {
            Kind::QuotedIdentifier | Kind::UnquotedIdentifier | Kind::Star => {
                self.expression(binding)
            }
            Kind::LBracket => {
                self.expect(Kind::LBracket)?;
                self.multi_select_list()
            }
            Kind::LBrace => {
                self.expect(Kind::LBrace)?;
                self.multi_select_hash()
            }
            kind => {
                let token = self.lookahead_token(0).clone();
                let message = format!(
                    "Expecting: ['quoted_identifier', 'unquoted_identifier', 'lbracket', 'lbrace'], got: {}",
                    kind.name()
                );
                Err(self.parse_error(&token, message))
            }
        }
    }

    fn expect(&mut self, kind: Kind) -> Result<(), Error> {
        if self.current() == kind {
            self.advance();
            Ok(())
        } else {
            let token = self.lookahead_token(0).clone();
            Err(self.expecting(kind.name(), &token))
        }
    }

    fn expecting(&self, expected: &str, token: &Token) -> Error {
        if token.kind == Kind::Eof {
            return self.incomplete();
        }
        let message = format!("Expecting: {expected}, got: {}", token.kind.name());
        self.parse_error(token, message)
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn current(&self) -> Kind {
        self.lookahead(0)
    }

    fn lookahead(&self, offset: isize) -> Kind {
        self.lookahead_token(offset).kind
    }

    /// Returns the token at the offset, which is the last token, i.e. the
    /// end of the expression, beyond the end.
    fn lookahead_token(&self, offset: isize) -> &Token {
        let index = self.index.saturating_add_signed(offset);
        self.tokens
            .get(index)
            .unwrap_or_else(|| self.tokens.last().expect("the end of the expression"))
    }

    fn parse_error(&self, token: &Token, message: String) -> Error {
        Error::Parse {
            position: token.start,
            value: token.text(),
            kind: token.kind.name().to_uppercase(),
            message,
            expression: self.expression.to_owned(),
        }
    }

    fn incomplete(&self) -> Error {
        Error::Incomplete {
            expression: self.expression.to_owned(),
        }
    }
}

fn number(token: &Token) -> i64 {
    match &token.value {
        Value::Int(digits) => digits.parse().unwrap_or(i64::MAX),
        _ => 0,
    }
}
