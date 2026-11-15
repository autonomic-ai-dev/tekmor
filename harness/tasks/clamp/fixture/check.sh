. ./lib/clamp.sh
[ "$(clamp 5 0 10)" = 5 ] || { echo "inside range"; exit 1; }
[ "$(clamp 15 0 10)" = 10 ] || { echo "above range"; exit 1; }
[ "$(clamp -3 0 10)" = 0 ] || { echo "below range"; exit 1; }
echo "clamp ok"
