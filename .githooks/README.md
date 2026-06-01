# Git Hooks

このディレクトリはローカル Git hook 用です。

有効化:

```bash
pnpm run hooks:install
```

`pre-push` は `review:strict` を実行し、失敗時は push を止めます。
