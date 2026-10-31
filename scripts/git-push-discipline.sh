#!/usr/bin/env bash
# 推送纪律（由 Lead 代行，人工亦可执行）：
#
#   【日常】dev 提交后：
#       git push aliyun dev                 # 阿里云私有备份，随时推
#       （可选）git push aliyun main        # main 在阿里云同留快照备份
#
#   【发版】打新版本标签后：
#       git tag -a Vx.y.z -m "..."          # 在 main 顶端打标签
#       git push aliyun main dev Vx.y.z     # 阿里云全量
#       git push github main Vx.y.z         # GitHub 只推 main + 标签，绝不推 dev
#
# 规则：
#   1. GitHub 只有 main 和发布标签，永远没有 dev。
#   2. main 只前进到「发布点」：日常合并进 main 的内容等于 dev 同步快照，
#      但对 GitHub 而言，两次 push 之间的 main 差异 = 一个新版本。
#   3. 本地 archive/main-history 是历史孤本，永不推送。
#
# 远端别名：
#   origin = git@github.com:mingwangyang/quicklook_linux.git   （发布目标）
#   aliyun = git@codeup.aliyun.com:6314b991cb9d006848799cac/quick_look_linux.git （私有备份）
#
# 注意：origin 这个名字历史上一直指 GitHub；为避免误推，
# 日常建议只用别名 aliyun / github 语义检查后再推 origin。
set -euo pipefail

cmd=${1:-status}
case "$cmd" in
  status)
    echo "== 本地分支 =="
    git branch -vv
    echo "== GitHub (origin) =="
    git ls-remote --heads --tags origin | sed 's|refs/||'
    echo "== 阿里云 (aliyun) =="
    git ls-remote --heads --tags aliyun | sed 's|refs/||'
    ;;
  daily)
    # 日常：dev -> 阿里云
    git push aliyun dev
    echo "[daily] dev pushed to aliyun."
    ;;
  release)
    # 发版：需要标签名。main/dev 应在同一点。
    tag=${2:?用法: git-push-discipline.sh release Vx.y.z}
    git push aliyun main dev "$tag"
    git push origin main "$tag"
    echo "[release] $tag -> aliyun(main,dev) + github(main only)."
    ;;
  *)
    echo "用法: $0 [status|daily|release Vx.y.z]" >&2
    exit 2
    ;;
esac
