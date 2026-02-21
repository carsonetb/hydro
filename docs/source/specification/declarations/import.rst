Imports
=======

Import declarations allow you to include code from other files into the 
current file. 

If ``import`` declarations are present in the file, they must 
all occur before any other declarations. For example, it is invalid to 
declare a class or function and *then* use an ``import`` statement.

Imports have a simple structure. They are identified with the ``import``
keyword. After that, a few things are valid: 

- If the module being imported is in the *top level* directory, then you simply
  type the name of the file without the extension.
- If the file being imported is in a package (subdirectory), you put in the 
  package path with each directory name seperated by a ``.`` symbol, and finally
  the name of the module.

Internal 
--------

When a module is imported, a static ``Module`` instance is created with 
the same name as the module. It contains members, similar to static 
members of a ``Type``, which represent the internal variables, classes,
and functions which are defined in the module. 

Example 
-------

The result is such that if a module called ``adder`` is created like so:

.. code-block:: c

    fn add(int lhs, int rhs) -> int = {
        return lhs + rhs;
    }

In a ``main`` module it can be imported:

.. code-block:: c

    import adder;

    fn main = {
        print(adder.add(2, 3)) // '5'
    }

AST for ``import`` 
------------------

.. code-block::

    importDecl = 'import' IDENTIFIER ( '.' IDENTIFIER )* ';'

Internal Representation
-----------------------

- path: ``list[str]``

Errors 
------

Syntax
++++++

- ERR00000: "Expected identifier after 'import' keyword."
- ERR00001: "Expected '.' or ';' after identifier in 'import' keyword."

Compile 
+++++++

- ERR10000: "Invalid import path '{path}'."