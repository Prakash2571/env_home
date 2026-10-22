import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Env Home",
  description: "Self-hosted home for environment variables",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body>{children}</body></html>;
}
