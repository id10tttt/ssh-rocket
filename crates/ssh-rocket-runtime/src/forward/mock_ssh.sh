#!/bin/bash
# 测试专用 SSH 替身：所有状态与输出均限制在单个测试的临时目录。
state_dir="${@: -1}"
direction=""
mapping=""
username=""
while (($#)); do
    case "$1" in
        -L|-R) direction="$1"; mapping="$2"; shift ;;
        -l) username="$2"; shift ;;
    esac
    shift
done
printf '%s\n' "$$" >> "$state_dir/attempts"
if [[ -z "$direction" ]]; then
    touch "$state_dir/unexpected_command"
    exit 99
fi
mode=$(<"$state_dir/mode")
case "$mode" in
    offline|conflict)
        printf '%s\n' "connect failed: $mode" >&2
        exit 255
        ;;
    hang)
        while true; do sleep 0.02; done
        ;;
    flood)
        for ((i=0; i<5000; i++)); do
            printf 'debug1: flood %080d\n' "$i" >&2
        done
        printf '%s\n' 'channel 1: open failed: connect failed: Connection refused' >&2
        ;;
esac
if [[ -f "$state_dir/allowed_user" && "$username" != "$(<"$state_dir/allowed_user")" ]]; then
    printf '%s\n' 'Permission denied (publickey).' >&2
    exit 255
fi
printf '%s\n' "$username" > "$state_dir/connected_user"
if [[ "$direction" == -R ]]; then
    IFS=: read -r remote_host remote_port local_host local_port <<< "$mapping"
    printf 'debug1: remote forward success for: listen %s:%s, connect %s:%s\n' \
        "$remote_host" "$remote_port" "$local_host" "$local_port" >&2
else
    printf '%s\n' 'debug1: Entering interactive session.' >&2
fi
while true; do
    mode=$(<"$state_dir/mode")
    if [[ "$mode" == offline || "$mode" == conflict ]]; then exit 255; fi
    sleep 0.02
done
