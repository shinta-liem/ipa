#!/usr/bin/env bash
#
# Generate 3 Docker images for each participant.
#
# This script optionally takes 3 arguments in the form `hostname`:`port` where hostname
# indicates the DNS name or IP address of the helper party and `port` is the open port on that instance.
#
# If arguments are not provided, `localhost` and `1443+$i` will be used by default ($i is in the range [1..3])
#
# This script "simulates" multi-stage Docker builds by adding extra layers to the container generated by
# `helper-image.sh` that copy public keys between 3 helpers and generate a valid configuration file. Those layers
# are then committed to the original image, overwriting it.
#
# As the final step, it generates 3 tar files for each of the image in the current directory.
#
# Prerequisites:
#
# - Docker CLI.
# - Docker daemon must be up and running.
#
# Examples:
#
# Create 3 images that listen to the port 80
# ./three-helper-images host1:80 host2:80 host3:80
#
# Create 3 images suitable to run on the local machine and listening to ports 1443 1444 and 1445
# ./three-helper-images

set -eou pipefail

help() {
  echo "Usage: $0 hostname:port"
  echo "- hostname: helper party public hostname"
  echo "- port: assigned port to communicate with this helper party"
  echo "Note: arguments are optional, if not specified \'localhost\' and \'443 + \$i\' will be used."
  echo "It is an error to supply more than 3 or less than 3 pairs."
}

rev=$(git log -n 1 --format='format:%H' | cut -c1-10)
copy_container_name="copy_container"
cleanup() {
  if docker ps -aq --filter "name=$copy_container_name" | grep -q .; then
    docker rm $copy_container_name > /dev/null
  fi
}
trap cleanup EXIT

# need to be consistent with the way we compute tag inside `helper-image.sh`.
image_tag() {
  local identity="$1"
  local tag="private-attribution/ipa:$rev-h$identity"
  echo "$tag"
}

copy_file() {
  local name="$1"
  local ext="$2"

  for ((src=0; src<=2; src++)); do
    for ((dst=src+1; dst<=src+2; dst++)); do
      from=$((src + 1))
      to=$((dst % 3 + 1))
      file="$name$from$ext"
      echo "copying $file from $from to $to"
      docker run --rm "$(image_tag $from)" cat "$file" \
        | docker run -i --name $copy_container_name "$(image_tag $to)" sh -c 'cat > '"$file" \
        && docker commit $copy_container_name "$(image_tag $to)" > /dev/null \
        && docker rm $copy_container_name > /dev/null || exit 1
    done;
  done;
}

cd "$(dirname "$0")" || exit 1

cleanup

hostnames=()
ports=()

# Process optional arguments
for arg in "$@"; do
  IFS=':' read -r host port <<< "$arg"
  hostnames+=("$host")
  ports+=("$port")
done

if [ ${#hostnames[@]} -eq 0 ]; then
  hostnames=(localhost localhost localhost)
  ports=(1443 1444 1445)
fi;

if (( ${#hostnames[@]} < 3 )) || (( ${#ports[@]} < 3 )); then
  help
  exit 1
fi


for i in "${!hostnames[@]}"; do
  echo "Generating image #$((i + 1)): host: ${hostnames[$i]}, port: ${ports[$i]}"
  ./helper-image.sh --hostname "${hostnames[$((i-1))]}" --identity "$((i + 1))"
done


# Copy TLS and mk public keys
copy_file "/etc/ipa/pub/h" ".pem"
copy_file "/etc/ipa/pub/h" "_mk.pub"

# generate network.toml
for ((i=1; i<=3; i++)); do
  # splitting is desirable here
  # shellcheck disable=SC2086
  docker run -i --name $copy_container_name  "$(image_tag "$i")" /usr/local/bin/ipa-helper confgen \
   --keys-dir /etc/ipa/pub \
   --hosts ${hostnames[*]} \
   --ports 443 443 443 \
    && docker commit $copy_container_name "$(image_tag "$i")" > /dev/null \
    && docker rm $copy_container_name > /dev/null || exit 1
done;

# make 3 tar files to upload them to the destinations
for ((i=1; i<=3; i++)); do
  docker save -o ipa-"$i".tar "$(image_tag "$i")"
done;



