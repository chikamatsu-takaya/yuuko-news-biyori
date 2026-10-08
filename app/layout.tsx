import type { Metadata } from 'next'
import { Noto_Sans_JP } from 'next/font/google'
import { Toaster } from "@/components/ui/toaster";
import UiThemeApplier from "@/components/layout/UiThemeApplier";
import './globals.css'

const notoSansJP = Noto_Sans_JP({ 
  subsets: ["latin"],
  weight: ["400", "500", "600", "700"],
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
    <html lang="ja" data-theme="default" className="bg-background">
      <body className={`${notoSansJP.className} antialiased`}>
        <UiThemeApplier />
        {children}
        <Toaster />
      </body>
    </html>
  )
}
