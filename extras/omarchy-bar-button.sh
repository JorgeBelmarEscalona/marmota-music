#!/bin/bash
# Now-playing chip for the Omarchy bar; click opens the Marmota Music notch.
state=$(marmota-music status 2>/dev/null)
title=$(jq -r '.title // empty' <<<"$state" 2>/dev/null)
if [[ -z $title ]]; then
  echo "♪"
  exit 0
fi
if [[ $(jq -r '.playing' <<<"$state") == "true" ]]; then
  echo "♪ $title"
else
  echo "⏸ $title"
fi
