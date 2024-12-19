## Purpose of the c_playground

Couchbase does not provide support for Rust bindings, so we cannot communicate code from this repo in support tickets.

With the c_playground, you can code whatever you want in C with full access to the couchbase-lite-C API.

## How to use it

The code in main.c will be compiled.

In the c_payground repository, you need to run the command once:

```
cmake CMakeLists.txt
```

You will then be able to compile the code anytime you want with the command:

```
make
```

The file Main is the executable, do not forget to set the execution right for yourself:

```
chmod u+x ./Main
```

Execute your code:

```
./Main
```
