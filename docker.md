# Working with docker
The enclave app and nodejs API server can be run using `docker-compose`,
from the root of the repository:

```shell
$ docker-compose up
```

## Developing the safetrace enclave code
There's a `docker-compose.yml`under [enclave/](enclave/) that can be used for
developing code. The source code is mounted in the container, and consequently
one can edit the files on the host, and run the tests in the container. For
instance:

```shell
$ cd enclave/
$ docker-compose run --rm enclave bash
root@76d0b45275b7:/usr/src/enclave/safetrace# cd app/
root@76d0b45275b7:/usr/src/enclave/safetrace/app# cargo test
...
```
