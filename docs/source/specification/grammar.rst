Hydro language grammar 
======================

.. code-block::

    // Bytes

    ALPHA = 
        | 'a'..'z'
        | 'A'..'Z'
        | '_'
    NUMERIC = '0'..'9'
    ALPHANUMERIC = ALPHA | NUMERIC
    IDENTIFIER = ALPHA ALPHANUMERIC*
    INT = NUMERIC+
    FLOAT = NUMERIC* ( '.' NUMERIC+ )?
    CHAR = ''' ANY '''
    STRING = '"' ANY* '"'

    // Pieces 

    type:         IDENTIFIER generics?
    type_name:    type IDENTIFIER

    generics:     '<' type ( ',' type )* '>'
    generics_def: '<' IDENTIFIER ( ':' IDENTIFIER )? ( ',' IDENTIFIER ( ':' IDENTIFIER )? )

    parameters:   type_name ( ',' type_name ) 
    inheritance:  ':' ( type | ( '(' type ( ',' type )* ')' ) )
    members:      declaration ( ',' declaration )*

    kwargs: IDENTIFIER '=' expression ( ',' IDENTIFIER '=' expression )*
    arguments: 
        | expression ( ',' expression )* kwargs?
        | kwargs

    annotation: 
        | '@' IDENTIFIER ( '(' arguments? ')' )?
        | IDENTIFIER
    
    assigner: 
        | '+='
        | '-='
        | '*='
        | '/='
        | '%='
        | '|='
        | '||='
        | '&='
        | '&&='
        | '^='
        | '**='
        | '<<='
        | '>>=' 
        | '@='
        | '='

    // Declarations

    import:   'import' IDENTIFIER ( '.' IDENTIFIER )* ';'
    varDecl:  IDENTIFIER generics? IDENTIFIER = expression ';'
    function: annotation* 'fn' IDENTIFIER generics_def? ( '(' parameters? ')' )? ( '->' type )? block
    class:    annotation* 'class' IDENTIFIER generics_def? inheritance? ( '(' parameters? ')' )? '{' declaration* '}'

    declaration: 
        | varDecl 
        | function 
        | class
    
    // Statements 

    varSet: IDENTIFIER assigner expression ';'
    kwStatement: IDENTIFIER ... // Defined at runtime

    statement:
        | kwStatement
        | varSet 
        | varDecl 
        | expression
    
    // Expressions 

    atom:
        | IDENTIFIER
        | INT 
        | FLOAT 
        | STRING 
        | CHARACTER
        | "true"
        | "false"
        | '{' scope? '}'
        | '(' expression | expressions? ')'
        | '[' expressions? ']'

    primary:
        | primary '.' IDENTIFIER 
        | primary generics? '(' arguments? ')'
        | primary '[' expression ']'
        | atom 

    power: primary ( "**" primary )*
    unary:
        | ( '!' | '-' ) unary 
        | power
    factor:      unary ( ( '*' | '/' | '%' | '@' ) unary )*
    term:        factor ( ( '+' | '-' ) factor )*
    shift_expr:  term ( ( "<<" | ">>" ) term )*
    bitwise_and: shift_expr ( '&' shift_expr )*
    bitwise_xor: bitwise_and ( '^' bitwise_and )*
    bitwise_or:  bitwise_xor ( '|' bitwise_xor )*
    comparison:  bitwise_or ( ( '<' | '>' | '<=' | '>=' ) bitwise_or )*
    equality:    comparison ( ( '==' | '!=' ) comparison )*
    conjunction: equality ( "&&" equality )*
    disjunction: conjunction ( "||" conjunction )*
    ternary:     disjunction ( '?' disjunction ':' disjunction )?
    expression:  ternary
    
    // Program 

    program: import* declaration*

Global statement grammar 
------------------------

.. code-block::

    if:     "if" "(" expression ")" "{" scope? "}" ( elifStmnt | elseStmnt )?
    elif:   "elif" "(" expression ")" "{" scope? "}" ( elifStmnt | elseStmnt )?
    else:   "else" "{" scope? "}"
    while:  "while" "(" expression ")" "{" scope? "}"
    for:    "for" "(" IDENTIFIER IDENTIFIER "in" expression ")" "{" scope? "}"
    return: "return" expression? ";"