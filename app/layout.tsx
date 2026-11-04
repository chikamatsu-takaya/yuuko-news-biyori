import type { Metadata } from 'next'
import { Noto_Sans_JP } from 'next/font/google'
import { Toaster } from "@/components/ui/toaster";
import UiThemeApplier from "@/components/layout/UiThemeApplier";
import './globals.css'

// Noto Sans JP は weight を指定せず可変フォント（weight 軸 100〜900）として読み込む（D134）。
// Google Fonts は 400/500/600/700 を指定しても同じ可変 woff2 を返すため、フォント本体は変わらないが、
// ウェイト指定だと unicode-range 分割（約124区画）の @font-face がウェイト数ぶん複製され、
// 描画をブロックする CSS が約4倍に膨らむ。見た目は同一で、font-normal〜bold はそのまま使える。
const notoSansJP = Noto_Sans_JP({
  subsets: ["latin"],
  variable: "--font-noto-sans-jp",
});

export const metadata: Metadata = {
  title: 'ゆうこのニュース日和 - ゆうこと、ニュースを読みやすく。',
  description: 'AIニュースマスコットアプリ - ゆうこと一緒にニュースを読もう',
  generator: 'v0.app',
  icons: {
    icon: [
      {
        url: '/icon-light-32x32.png',
        media: '(prefers-color-scheme: light)',
      },
      {
        url: '/icon-dark-32x32.png',
        media: '(prefers-color-scheme: dark)',
      },
      {
        url: '/icon.svg',
        type: 'image/svg+xml',
      },
    ],
    apple: '/apple-icon.png',
  },
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    // 配色は data-theme で切り替わる CSS 変数（app/globals.css）に従わせる。
    // 初期 HTML は既定テーマで描き、保存済みテーマは UiThemeApplier がマウント後に反映する。
    // そのため別テーマ選択時は、起動直後の最初の描画だけ一瞬既定色（クリーム）になる。
    // data-pattern はテーマの地に敷く模様（D152〜D154）。既定は "none"（模様なし）で、メイン画面だけ
    // components/layout/PatternGround.tsx が保存済みの模様を反映する（常駐ゆうこウィンドウは "none" のまま）。
    <html lang="ja" data-theme="default" data-pattern="none" className="bg-background">
      <body className={`${notoSansJP.className} antialiased`}>
        <UiThemeApplier />
        {children}
        <Toaster />
      </body>
    </html>
  )
}
