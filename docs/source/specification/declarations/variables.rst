Top level variables 
===================

Top level variables are global and accessible from any scope in the 
module. 

.. warning:: 
    Generally using global variables is unadvised, because having a 
    mutable global state can cause inconsistency in functions that 
    utilize and modify those global variables. 

The parser will interpret any identifier that is not ``import``, 
``class``, or ``fn`` to be a variable declaration.

Example 
-------

Here is an example of using global variables inside the ``main`` 
function:

.. code-block:: c

    int x = 0;

    fn main = {
        print(x + 1); // '1'
        x = 5;
        print(x); // '5'
    }

AST
---

.. code-block:: 

    IDENTIFIER generics? IDENTIFIER = expression ';'

Internal representation
-----------------------

- type: ``Identifier``
- name: ``Identifier``
- value: ``Expression``

Errors 
------

Syntax
++++++

- ERR00100: "Expected variable name after variable type '{type}'."
- ERR00101: "Expected '=' after variable name '{name}'."
- ERR00102: "Expected ';' after expression in variable declaration."

Compile 
+++++++

- ERR10100: "'{name}' is already a global identifier."