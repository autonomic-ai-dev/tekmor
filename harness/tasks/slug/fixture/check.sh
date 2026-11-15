. ./lib/slug.sh
[ "$(slug 'Hello World')" = 'hello-world' ] || { echo "spaces"; exit 1; }
[ "$(slug 'ABC')" = 'abc' ] || { echo "case"; exit 1; }
echo "slug ok"
