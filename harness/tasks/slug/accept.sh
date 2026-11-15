set -eu
cd "$1"
. ./lib/slug.sh
[ "$(slug 'Hello World')" = 'hello-world' ]
[ "$(slug 'ABC')" = 'abc' ]
[ "$(slug '  Many   Spaces  ')" = 'many-spaces' ]
[ "$(slug 'Keep-Dashes_and.dots!')" = 'keep-dashes-and-dots' ]
[ "$(slug 'a')" = 'a' ]
echo "accept ok"
