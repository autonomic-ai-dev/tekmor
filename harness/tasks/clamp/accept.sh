set -eu
cd "$1"
. ./lib/clamp.sh
[ "$(clamp 5 0 10)" = 5 ]
[ "$(clamp 15 0 10)" = 10 ]
[ "$(clamp -3 0 10)" = 0 ]
[ "$(clamp 0 0 10)" = 0 ]
[ "$(clamp 10 0 10)" = 10 ]
[ "$(clamp 7 7 7)" = 7 ]
[ "$(clamp -100 -5 5)" = -5 ]
echo "accept ok"
