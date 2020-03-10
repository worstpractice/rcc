extern crate itertools;
use crate::tokenizer::{Keyword, TokNLoc, Token};
use core::slice::Iter;
use itertools::MultiPeek;
use std::error;
use std::fmt;

use crate::ast::{AssignmentKind, BinaryOp, FixOp, UnaryOp};
use crate::ast::{BlockItem, Declaration, Expression, Function, Program, Statement};

//===================================================================
// Parsing
//===================================================================

#[derive(Debug, Clone)]
pub struct ParseError {
    pub cursor: usize,
    pub message: String,
}

impl ParseError {
    fn new(cursor: usize, message: String) -> ParseError {
        ParseError { cursor, message }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ParseError {}: {}", self.cursor, self.message)
    }
}

impl error::Error for ParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

fn mkperr(tok: TokNLoc, msg: &str) -> ParseError {
    ParseError { cursor: tok.location, message: format!("{}, got '{}'.", msg, tok.token) }
}

pub struct Parser<'a> {
    tokiter: MultiPeek<Iter<'a, TokNLoc>>,
}

impl Parser<'_> {
    fn new(tokens: &[TokNLoc]) -> Parser {
        Parser { tokiter: itertools::multipeek(tokens.iter()) }
    }

    fn next(&mut self) -> Option<TokNLoc> {
        match self.tokiter.next() {
            Some(t) => Some(t.clone()),
            None => None,
        }
    }

    fn peek_n(&mut self, n: u8) -> Option<TokNLoc> {
        let mut p = None;
        for _i in 0..n {
            p = self.tokiter.peek();
        }
        let res = match p {
            Some(t) => Some((*t).clone()),
            None => None,
        };
        self.tokiter.reset_peek();
        res
    }

    fn peek(&mut self) -> Option<TokNLoc> {
        self.peek_n(1)
    }

    fn assert_next_token(&mut self, matches: impl Fn(&Token) -> bool, msg: &str) -> Result<(), ParseError> {
        let tok = self.next().unwrap();
        if matches(&tok.token) {
            Ok(())
        } else {
            Err(mkperr(tok, msg))
        }
    }

    fn ensure_semicolon(&mut self, msg: &str) -> Result<(), ParseError> {
        // ensure last token is a semicolon
        self.assert_next_token(|t| matches!(t, Token::Semicolon), &format!("{}. Expected a final semicolon", msg))
    }

    fn parse_postfix_expression(&mut self) -> Result<Expression, ParseError> {
        let tok = self.next().unwrap();
        match tok.token {
            Token::Lparen => {
                let subexpr = self.parse_expression()?;
                self.assert_next_token(|t| matches!(t, Token::Rparen), "Missing closing parenthesis after expression")?;
                Ok(subexpr)
            }
            Token::Identifier(id) => {
                match self.peek().unwrap().token {
                    Token::Increment => {
                        self.next(); // consume
                        Ok(Expression::PostfixOp(FixOp::Inc, id))
                    }
                    Token::Decrement => {
                        self.next(); // consume
                        Ok(Expression::PostfixOp(FixOp::Dec, id))
                    }
                    Token::Lparen => {
                        self.next(); // consume

                        let mut args = Vec::new();

                        if !matches!(self.peek().unwrap().token, Token::Rparen) {
                            loop {
                                args.push(self.parse_expression()?);

                                match self.peek().unwrap().token {
                                    Token::Comma => {
                                        self.next();
                                    } // consume
                                    _ => break,
                                }
                            }
                        }

                        self.assert_next_token(
                            |t| matches!(t, Token::Rparen),
                            "Missing closing parenthesis function arguments",
                        )?;

                        Ok(Expression::FunctionCall(id, args))
                    }
                    _ => Ok(Expression::Variable(id)),
                }
            }
            Token::IntLiteral(v) => Ok(Expression::Constant(v)),
            _ => Err(mkperr(
                tok,
                "Invalid postfix expression. \
                                 Expected int literal, (expr), or identifier \
                                 possibly with postfix operator",
            )),
        }
    }

    fn parse_prefix_expression(&mut self) -> Result<Expression, ParseError> {
        let tok = self.peek().unwrap();
        match tok.token {
            Token::Minus => {
                self.next(); // consume
                let operand = self.parse_prefix_expression()?;
                Ok(Expression::UnaryOp(UnaryOp::Negate, Box::new(operand)))
            }
            Token::Not => {
                self.next(); // consume
                let operand = self.parse_prefix_expression()?;
                Ok(Expression::UnaryOp(UnaryOp::Not, Box::new(operand)))
            }
            Token::Complement => {
                self.next(); // consume
                let operand = self.parse_prefix_expression()?;
                Ok(Expression::UnaryOp(UnaryOp::Complement, Box::new(operand)))
            }
            Token::Increment => {
                self.next(); // consume

                let next_loc = self.peek().unwrap().location; // for mkperr message

                let operand = self.parse_postfix_expression()?;
                if let Expression::Variable(id) = operand {
                    Ok(Expression::PrefixOp(FixOp::Inc, id))
                } else {
                    Err(ParseError::new(
                        next_loc,
                        "Invalid prefix expression. Expected variable identifier after prefix increment/decrement"
                            .to_string(),
                    ))
                }
            }
            Token::Decrement => {
                self.next(); // consume

                let next_loc = self.peek().unwrap().location; // for mkperr message

                let operand = self.parse_postfix_expression()?;
                if let Expression::Variable(id) = operand {
                    Ok(Expression::PrefixOp(FixOp::Dec, id))
                } else {
                    Err(ParseError::new(
                        next_loc,
                        "Invalid prefix expression. Expected variable identifier after prefix increment/decrement"
                            .to_string(),
                    ))
                }
            }
            _ => self.parse_postfix_expression(),
        }
    }

    fn parse_binary_expression<P, T>(
        &mut self,
        parse_operand: P,
        token_to_operation: T,
    ) -> Result<Expression, ParseError>
    where
        P: Fn(&mut Parser) -> Result<Expression, ParseError>,
        T: Fn(Token) -> Option<BinaryOp>,
    {
        let mut operand = parse_operand(self)?;

        while let Some(tok) = self.peek() {
            let optop = token_to_operation(tok.token);

            if let Some(op) = optop {
                self.next(); // consume
                let next_operand = parse_operand(self)?;
                operand = Expression::BinaryOp(op, Box::new(operand), Box::new(next_operand));
            } else {
                break;
            }
        }

        Ok(operand)
    }

    fn parse_multiplicative_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_factor = |parser: &mut Parser| parser.parse_prefix_expression();
        let token_to_multiplicative_op = |tok| match tok {
            Token::Multiplication => Some(BinaryOp::Multiplication),
            Token::Division => Some(BinaryOp::Division),
            Token::Remainder => Some(BinaryOp::Remainder),
            _ => None,
        };

        self.parse_binary_expression(parse_factor, token_to_multiplicative_op)
    }

    fn parse_additive_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_term = |parser: &mut Parser| parser.parse_multiplicative_expression();
        let token_to_additive_op = |tok| match tok {
            Token::Minus => Some(BinaryOp::Subtraction),
            Token::Plus => Some(BinaryOp::Addition),
            _ => None,
        };

        self.parse_binary_expression(parse_term, token_to_additive_op)
    }

    fn parse_shift_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_addexpr = |parser: &mut Parser| parser.parse_additive_expression();
        let token_to_shift_op = |tok| match tok {
            Token::LeftShift => Some(BinaryOp::LeftShift),
            Token::RightShift => Some(BinaryOp::RightShift),
            _ => None,
        };

        self.parse_binary_expression(parse_addexpr, token_to_shift_op)
    }

    fn parse_relational_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_shiftexpr = |parser: &mut Parser| parser.parse_shift_expression();
        let token_to_relational_op = |tok| match tok {
            Token::Greater => Some(BinaryOp::Greater),
            Token::Less => Some(BinaryOp::Less),
            Token::GreaterEqual => Some(BinaryOp::GreaterEqual),
            Token::LessEqual => Some(BinaryOp::LessEqual),
            _ => None,
        };

        self.parse_binary_expression(parse_shiftexpr, token_to_relational_op)
    }

    fn parse_equality_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_relexpr = |parser: &mut Parser| parser.parse_relational_expression();
        let token_to_equality_op = |tok| match tok {
            Token::Equal => Some(BinaryOp::Equal),
            Token::NotEqual => Some(BinaryOp::NotEqual),
            _ => None,
        };

        self.parse_binary_expression(parse_relexpr, token_to_equality_op)
    }

    fn parse_bitwise_and_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_eqexpr = |parser: &mut Parser| parser.parse_equality_expression();
        let token_to_bitwise_and_op = |tok| match tok {
            Token::BitwiseAnd => Some(BinaryOp::BitwiseAnd),
            _ => None,
        };

        self.parse_binary_expression(parse_eqexpr, token_to_bitwise_and_op)
    }

    fn parse_bitwise_xor_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_bitandexpr = |parser: &mut Parser| parser.parse_bitwise_and_expression();
        let token_to_bitwise_xor_op = |tok| match tok {
            Token::BitwiseXor => Some(BinaryOp::BitwiseXor),
            _ => None,
        };

        self.parse_binary_expression(parse_bitandexpr, token_to_bitwise_xor_op)
    }

    fn parse_bitwise_or_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_bitxorexpr = |parser: &mut Parser| parser.parse_bitwise_xor_expression();
        let token_to_bitwise_or_op = |tok| match tok {
            Token::BitwiseOr => Some(BinaryOp::BitwiseOr),
            _ => None,
        };

        self.parse_binary_expression(parse_bitxorexpr, token_to_bitwise_or_op)
    }

    fn parse_logical_and_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_bitorexpr = |parser: &mut Parser| parser.parse_bitwise_or_expression();
        let token_to_logical_and_op = |tok| match tok {
            Token::LogicalAnd => Some(BinaryOp::LogicalAnd),
            _ => None,
        };

        self.parse_binary_expression(parse_bitorexpr, token_to_logical_and_op)
    }

    fn parse_logical_or_expression(&mut self) -> Result<Expression, ParseError> {
        let parse_logandexpr = |parser: &mut Parser| parser.parse_logical_and_expression();
        let token_to_logical_or_op = |tok| match tok {
            Token::LogicalOr => Some(BinaryOp::LogicalOr),
            _ => None,
        };

        self.parse_binary_expression(parse_logandexpr, token_to_logical_or_op)
    }

    fn parse_conditional_expression(&mut self) -> Result<Expression, ParseError> {
        let loexpr = self.parse_logical_or_expression()?;

        if let Token::QuestionMark = &self.peek().unwrap().token {
            self.next(); // consume

            let ifexpr = self.parse_expression()?;

            self.assert_next_token(|t| matches!(t, Token::Colon), "Invalid conditional statement. Expected ':'")?;

            let elseexpr = self.parse_conditional_expression()?;

            Ok(Expression::Conditional(Box::new(loexpr), Box::new(ifexpr), Box::new(elseexpr)))
        } else {
            Ok(loexpr)
        }
    }

    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        if let Token::Identifier(id) = &self.peek().unwrap().token {
            let ass = match self.peek_n(2).unwrap().token {
                Token::Assignment => Some(AssignmentKind::Write),
                Token::AdditionAssignment => Some(AssignmentKind::Add),
                Token::SubtractionAssignment => Some(AssignmentKind::Subtract),
                Token::MultiplicationAssignment => Some(AssignmentKind::Multiply),
                Token::DivisionAssignment => Some(AssignmentKind::Divide),
                Token::RemainderAssignment => Some(AssignmentKind::Remainder),
                Token::BitwiseXorAssignment => Some(AssignmentKind::BitwiseXor),
                Token::BitwiseOrAssignment => Some(AssignmentKind::BitwiseOr),
                Token::BitwiseAndAssignment => Some(AssignmentKind::BitwiseAnd),
                Token::LeftShiftAssignment => Some(AssignmentKind::LeftShift),
                Token::RightShiftAssignment => Some(AssignmentKind::RightShift),
                _ => None,
            };

            if let Some(asskind) = ass {
                self.next(); // consume twice
                self.next();
                let expr = self.parse_expression()?;
                return Ok(Expression::Assign(asskind, id.to_string(), Box::new(expr)));
            }
        }

        self.parse_conditional_expression()
    }

    fn parse_compound_statement(&mut self) -> Result<Vec<BlockItem>, ParseError> {
        // ensure next token is '{'
        self.assert_next_token(|t| matches!(t, Token::Lbrace), "Invalid compound statement. Expected '{{'")?;

        // parse block items
        let mut block_items = Vec::new();

        loop {
            if let Token::Rbrace = self.peek().unwrap().token {
                break;
            }
            block_items.push(self.parse_block_item()?);
        }

        // we know next token is '}'
        // simply consume
        self.next();

        Ok(block_items)
    }

    fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let stmt = match self.peek().unwrap().token {
            Token::Lbrace => {
                let comp = self.parse_compound_statement()?;
                Statement::Compound(comp)
            }
            Token::Keyword(Keyword::Return) => {
                self.next(); // consume
                let expr = self.parse_expression()?;
                self.ensure_semicolon("Invalid return statement")?;
                Statement::Return(expr)
            }
            Token::Keyword(Keyword::Continue) => {
                self.next(); // consume
                self.ensure_semicolon("Invalid continue statement")?;
                Statement::Continue
            }
            Token::Keyword(Keyword::Break) => {
                self.next(); // consume
                self.ensure_semicolon("Invalid break statement")?;
                Statement::Break
            }
            Token::Keyword(Keyword::If) => {
                self.next(); // consume

                // ensure next token is '('
                self.assert_next_token(|t| matches!(t, Token::Lparen), "Invalid if statement. Expected '('")?;

                let cond_expr = self.parse_expression()?;

                // ensure next token is ')'
                self.assert_next_token(
                    |t| matches!(t, Token::Rparen),
                    "Invalid if statement. Expected ')' after condition expression",
                )?;

                let if_stmnt = self.parse_statement()?;

                if let Token::Keyword(Keyword::Else) = self.peek().unwrap().token {
                    self.next(); // consume

                    let else_stmnt = self.parse_statement()?;

                    Statement::If(cond_expr, Box::new(if_stmnt), Some(Box::new(else_stmnt)))
                } else {
                    Statement::If(cond_expr, Box::new(if_stmnt), None)
                }
            }
            Token::Keyword(Keyword::While) => {
                self.next(); // consume

                // ensure next token is '('
                self.assert_next_token(|t| matches!(t, Token::Lparen), "Invalid while statement. Expected '('")?;

                let cond_expr = self.parse_expression()?;

                // ensure next token is ')'
                self.assert_next_token(
                    |t| matches!(t, Token::Rparen),
                    "Invalid while statement. Expected ')' after condition expression",
                )?;

                let body = self.parse_statement()?;

                Statement::While(cond_expr, Box::new(body))
            }
            Token::Keyword(Keyword::Do) => {
                self.next(); // consume

                let body = self.parse_statement()?;

                // ensure next token is 'while'
                self.assert_next_token(
                    |t| matches!(t, Token::Keyword(Keyword::While)),
                    "Invalid do-while statement. Expected 'while'",
                )?;

                // ensure next token is '('
                self.assert_next_token(|t| matches!(t, Token::Lparen), "Invalid do-while statement. Expected '('")?;

                let cond_expr = self.parse_expression()?;

                // ensure next token is ')'
                self.assert_next_token(
                    |t| matches!(t, Token::Rparen),
                    "Invalid do-while statement. Expected ')' after condition expression",
                )?;

                self.ensure_semicolon("Invalid do-while statement")?;

                Statement::DoWhile(Box::new(body), cond_expr)
            }
            Token::Keyword(Keyword::For) => {
                self.next(); // consume

                // ensure next token is '('
                self.assert_next_token(|t| matches!(t, Token::Lparen), "Invalid For/ForDecl statement. Expected '('")?;

                let mut init_decl: Option<Declaration> = None;
                let mut init_expr: Option<Expression> = None;

                let tok = self.peek().unwrap();
                match tok.token {
                    Token::Keyword(Keyword::Int) => {
                        init_decl = Some(self.parse_declaration()?);
                        // no need to look for ';', it is included in declaration
                    }
                    Token::Semicolon => {
                        self.next(); // consume
                    }
                    _ => {
                        init_expr = Some(self.parse_expression()?);
                        self.ensure_semicolon("Invalid initialization expression for For statement")?;
                    }
                }

                let cond_expr = if let Token::Semicolon = self.peek().unwrap().token {
                    // no conditional expression - generate a constant '1'
                    Expression::Constant(1)
                } else {
                    self.parse_expression()?
                };

                self.ensure_semicolon("Invalid condition expression for ForDecl statement")?;

                let post_expr = if let Token::Rparen = self.peek().unwrap().token {
                    // no post_expr, Rparen read below
                    None
                } else {
                    let pexpr = self.parse_expression()?;
                    Some(pexpr)
                };

                // ensure next token is ')'
                self.assert_next_token(
                    |t| matches!(t, Token::Rparen),
                    "Invalid For/ForDecl statement. Expected ')' after post expression",
                )?;

                let body = self.parse_statement()?;

                if let Some(decl) = init_decl {
                    Statement::ForDecl(decl, cond_expr, post_expr, Box::new(body))
                } else {
                    Statement::For(init_expr, cond_expr, post_expr, Box::new(body))
                }
            }
            _ => {
                // then we have an expression to parse

                if let Token::Semicolon = self.peek().unwrap().token {
                    self.tokiter.next(); // consume
                    Statement::Null
                } else {
                    let expr = self.parse_expression()?;
                    self.ensure_semicolon("Invalid expression statement")?;
                    Statement::Expr(expr)
                }
            }
        };

        Ok(stmt)
    }

    fn parse_declaration(&mut self) -> Result<Declaration, ParseError> {
        // ensure we got a type (i.e. 'int')
        self.assert_next_token(
            |t| matches!(t, Token::Keyword(Keyword::Int)),
            "Invalid declaration. Expected type specifier",
        )?;

        let mut tok = self.next().unwrap();
        let id = match tok.token {
            Token::Identifier(n) => n,
            _ => {
                return Err(mkperr(tok, "Invalid declaration. Expected an identifier"));
            }
        };

        // parse initialization if next token is an assignment (equals sign)
        tok = self.peek().unwrap();
        let init = match tok.token {
            Token::Assignment => {
                self.next(); // consume
                Some(self.parse_expression()?)
            }
            _ => None,
        };

        // ensure last token is a semicolon
        self.ensure_semicolon("Invalid declaration")?;

        Ok(Declaration { id, init })
    }

    fn parse_block_item(&mut self) -> Result<BlockItem, ParseError> {
        let bkitem = match self.peek().unwrap().token {
            Token::Keyword(Keyword::Int) => {
                let declaration = self.parse_declaration()?;

                BlockItem::Decl(declaration)
            }
            _ => {
                // then we have an expression to parse
                BlockItem::Stmt(self.parse_statement()?)
            }
        };

        Ok(bkitem)
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        // ensure first token is an Int keyword
        self.assert_next_token(
            |t| matches!(t, Token::Keyword(Keyword::Int)),
            "Invalid function declarator. Expected return type",
        )?;

        // next token should be an identifier
        let tok = self.next().unwrap();
        let function_name = match tok.token {
            Token::Identifier(ident) => ident,
            _ => {
                return Err(mkperr(tok, "Invalid function declarator. Expected identifier for function name"));
            }
        };

        // ensure next token is '('
        self.assert_next_token(|t| matches!(t, Token::Lparen), "Invalid function declarator. Expected '('")?;

        // parse the parameter ids
        let mut parameter_list = Vec::new();

        if let Token::Keyword(Keyword::Int) = self.peek().unwrap().token {
            self.next(); // consume parameter type (int)

            // read parameter id
            let tok = self.next().unwrap();
            match tok.token {
                Token::Identifier(id) => parameter_list.push(id),
                _ => return Err(mkperr(tok, "Invalid function declarator. Expected identifier")),
            }

            while let Token::Comma = self.peek().unwrap().token {
                self.next(); // consume comma

                // ensure next token is 'int'
                self.assert_next_token(
                    |t| matches!(t, Token::Keyword(Keyword::Int)),
                    "Invalid function declarator. Expected type after comma",
                )?;

                let tok = self.next().unwrap();
                match tok.token {
                    Token::Identifier(id) => {
                        parameter_list.push(id);
                    }
                    _ => {
                        return Err(mkperr(tok, "Invalid function parameter list. Expected identifier after type"));
                    }
                }
            }
        }

        // ensure next token is ')'
        self.assert_next_token(|t| matches!(t, Token::Rparen), "Invalid function declarator. Expected ')'")?;

        if let Token::Semicolon = self.peek().unwrap().token {
            self.next(); // consume semicolon
            Ok(Function::Declaration(function_name, parameter_list))
        } else {
            // parse body
            let body = self.parse_compound_statement()?;

            Ok(Function::Definition(function_name, parameter_list, body))
        }
    }

    fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut functions = Vec::new();
        while let Some(_) = self.peek() {
            functions.push(self.parse_function()?);
        }
        Ok(Program::Prog(functions))
    }
}

pub fn parse(tokens: &[TokNLoc]) -> Result<Program, ParseError> {
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}
