import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "FlashStat Dashboard",
  description:
    "Real-time Unichain Flashblock confidence and sequencer reputation monitor",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
