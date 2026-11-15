clamp() {
  local v=$1 lo=$2 hi=$3
  if [ "$v" -lt "$lo" ]; then
    echo "$hi"
  elif [ "$v" -gt "$hi" ]; then
    echo "$lo"
  else
    echo "$v"
  fi
}
