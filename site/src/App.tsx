import type { ReactNode } from "react"
import {
  ArrowUp,
  Laptop,
  Check,
  Globe,
  Keyboard,
  Layers,
  MousePointerClick,
  Palette,
  Plug,
  Usb,
  Zap,
} from "lucide-react"

import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"

const APP_URL = import.meta.env.VITE_APP_URL ?? "https://aula75-app.rosman21.workers.dev/"
const DMG = "/download/AULA-75-Control.dmg"

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="inline-flex min-w-6 items-center justify-center rounded-md border border-b-2 bg-muted px-1.5 py-0.5 font-mono text-xs font-semibold text-foreground">
      {children}
    </kbd>
  )
}

function Section({ id, title, sub, children }: { id?: string; title: string; sub?: string; children: ReactNode }) {
  return (
    <section id={id} className="mx-auto max-w-6xl scroll-mt-20 px-6 pt-24">
      <h2 className="text-3xl font-bold tracking-tight sm:text-4xl">{title}</h2>
      {sub && <p className="mt-3 max-w-2xl text-lg text-muted-foreground">{sub}</p>}
      <div className="mt-10">{children}</div>
    </section>
  )
}

function Logo() {
  return (
    <span className="grid size-7 place-items-center rounded-lg bg-gradient-to-br from-blue-600 to-violet-500 text-xs font-extrabold text-white">
      75
    </span>
  )
}

function Nav() {
  const links = [
    ["How it works", "#how"],
    ["Features", "#features"],
    ["Get it", "#get"],
    ["FAQ", "#faq"],
  ]
  return (
    <nav className="sticky top-0 z-20 border-b bg-background/80 backdrop-blur-xl">
      <div className="mx-auto flex h-14 max-w-6xl items-center gap-6 px-6">
        <a href="#" className="flex items-center gap-2.5 font-semibold">
          <Logo />
          AULA 75 Control
        </a>
        <div className="ml-auto hidden items-center gap-1 md:flex">
          {links.map(([label, href]) => (
            <Button key={href} variant="ghost" size="sm" asChild className="text-muted-foreground">
              <a href={href}>{label}</a>
            </Button>
          ))}
        </div>
        <Button size="sm" asChild className="ml-auto md:ml-2">
          <a href={DMG}>Download</a>
        </Button>
      </div>
    </nav>
  )
}

function Hero() {
  return (
    <header className="relative overflow-x-clip">
      <div className="pointer-events-none absolute inset-x-0 -top-40 -z-10 mx-auto h-[480px] max-w-4xl rounded-full bg-gradient-to-r from-blue-500/20 via-violet-500/20 to-sky-400/20 blur-3xl" />
      <div className="mx-auto max-w-6xl px-6 pt-20 pb-8 text-center sm:pt-28">
        <Badge variant="secondary" className="mb-6 gap-1.5 rounded-full px-3 py-1 whitespace-normal text-primary">
          <Usb className="size-3.5" /> For the AULA F75 · 2.4 GHz receiver or USB-C
        </Badge>
        <h1 className="text-5xl font-bold tracking-tighter text-balance sm:text-7xl">
          Your keyboard,
          <br />
          your keys.
        </h1>
        <p className="mx-auto mt-6 max-w-2xl text-lg text-pretty text-muted-foreground sm:text-xl">
          Remap any key with real Mac names, turn keys into shortcuts and macros, and pick the lighting — without
          the vendor driver. Click a key, click what it should do, done.
        </p>
        <div className="mt-9 flex flex-wrap justify-center gap-3">
          <Button size="lg" asChild className="h-12 rounded-xl px-6 text-base">
            <a href={DMG} id="download">
              <Laptop className="size-5" /> Download for Mac
              <span className="hidden text-xs font-normal opacity-75 sm:inline">Apple silicon · macOS 14+</span>
            </a>
          </Button>
          <Button size="lg" variant="outline" asChild className="h-12 rounded-xl px-6 text-base">
            <a href={APP_URL} id="open-web">
              <Globe className="size-5" /> Open in Chrome
              <span className="hidden text-xs font-normal text-muted-foreground sm:inline">no install</span>
            </a>
          </Button>
        </div>
        <p className="mt-4 text-sm text-muted-foreground">
          Free. Everything is written to the keyboard itself, so it keeps working on any computer.
        </p>

        <div className="mx-auto mt-16 max-w-5xl overflow-hidden rounded-2xl border bg-card shadow-2xl shadow-black/15">
          <div className="flex h-8 items-center gap-2 border-b bg-muted/60 px-3">
            <span className="size-3 rounded-full bg-[#ff5f57]" />
            <span className="size-3 rounded-full bg-[#febc2e]" />
            <span className="size-3 rounded-full bg-[#28c840]" />
          </div>
          <img
            src="/img/keys.png"
            alt="The Keys screen: the left Command and Option keys swapped, each showing what it was"
            width={1180}
            height={720}
            className="block h-auto w-full"
          />
        </div>
      </div>
    </header>
  )
}

const steps = [
  {
    icon: Plug,
    title: "Plug it in",
    body: (
      <>
        Put the 2.4 GHz receiver in a USB port and set the keyboard's switch to <b>2.4G</b>. Or use the USB-C cable
        with the switch on <b>wired</b>.
      </>
    ),
  },
  {
    icon: Usb,
    title: "Connect",
    body: (
      <>
        On a Mac, allow Input Monitoring once. In Chrome, choose <b>Connect keyboard</b> and pick “2.4G Wireless
        Receiver”.
      </>
    ),
  },
  {
    icon: MousePointerClick,
    title: "Click and change",
    body: <>Click a key on the picture, then click what it should become. It's saved to the keyboard at once.</>,
  },
]

function HowItWorks() {
  return (
    <Section
      id="how"
      title="How it works"
      sub="The app talks to the keyboard over the same USB link AULA's own driver uses. Changes are stored on the keyboard, not on your computer."
    >
      <div className="grid gap-5 md:grid-cols-3">
        {steps.map((s, i) => (
          <Card key={s.title} className="gap-3">
            <CardHeader>
              <div className="mb-2 flex items-center gap-3">
                <span className="grid size-9 place-items-center rounded-full bg-primary text-sm font-bold text-primary-foreground">
                  {i + 1}
                </span>
                <s.icon className="size-5 text-muted-foreground" />
              </div>
              <CardTitle className="text-lg">{s.title}</CardTitle>
            </CardHeader>
            <CardContent className="text-[15px] text-muted-foreground">{s.body}</CardContent>
          </Card>
        ))}
      </div>
    </Section>
  )
}

const features = [
  {
    icon: Keyboard,
    title: "Remap any key",
    body: (
      <>
        Swap <Kbd>⌘</Kbd> and <Kbd>⌥</Kbd>, turn Caps Lock into <Kbd>Esc</Kbd>, or move Fn's media keys. Left and
        right modifiers are separate.
      </>
    ),
  },
  {
    icon: Zap,
    title: "Shortcuts on one key",
    body: (
      <>
        Turn on <Kbd>⌘</Kbd> <Kbd>⌥</Kbd> <Kbd>⌃</Kbd> <Kbd>⇧</Kbd>, then click a key: Fn + A can send{" "}
        <Kbd>⌘C</Kbd>.
      </>
    ),
  },
  {
    icon: Layers,
    title: "Macros",
    body: <>Type text, press combinations and wait between steps. Play once, while held, or toggle on and off.</>,
  },
  {
    icon: Palette,
    title: "Lighting",
    body: <>Every built-in effect with brightness, speed and colour, applied live as you change it, plus the sleep timer.</>,
  },
]

function Features() {
  return (
    <Section id="features" title="Everything the F75 can do" sub="Built around the way VIA and Vial work, made for the AULA F75.">
      <div className="grid gap-5 sm:grid-cols-2">
        {features.map((f) => (
          <Card key={f.title} className="gap-3 transition-shadow hover:shadow-lg">
            <CardHeader>
              <div className="mb-2 grid size-10 place-items-center rounded-xl bg-primary/10 text-primary">
                <f.icon className="size-5" />
              </div>
              <CardTitle className="text-xl">{f.title}</CardTitle>
            </CardHeader>
            <CardContent className="text-[15px] leading-relaxed text-muted-foreground">{f.body}</CardContent>
          </Card>
        ))}
      </div>
    </Section>
  )
}

function Picker() {
  return (
    <section className="mx-auto grid max-w-6xl items-center gap-10 px-6 pt-24 md:grid-cols-2">
      <div>
        <h2 className="text-3xl font-bold tracking-tight sm:text-4xl">One click per key.</h2>
        <p className="mt-3 text-lg text-muted-foreground">
          Pick a key and the whole keyboard shows up again as the menu of what it can become, with media, navigation
          and macros below. No long dropdowns.
        </p>
      </div>
      <img
        src="/img/picker.png"
        alt="Choosing what Caps Lock does from a keyboard of options"
        loading="lazy"
        className="w-full rounded-2xl border shadow-xl shadow-black/10"
      />
    </section>
  )
}

function Point({ children }: { children: ReactNode }) {
  return (
    <li className="flex gap-2.5">
      <Check className="mt-0.5 size-4 shrink-0 text-primary" />
      <span>{children}</span>
    </li>
  )
}

function GetIt() {
  return (
    <Section id="get" title="Two ways to use it" sub="The same app, on your Mac or in your browser.">
      <div className="grid gap-5 md:grid-cols-2">
        <Card className="border-primary/40 bg-gradient-to-b from-primary/5 to-card">
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-2xl">
              <Laptop className="size-6" /> Mac app
            </CardTitle>
            <CardDescription>Recommended on a Mac.</CardDescription>
          </CardHeader>
          <CardContent>
            <ul className="space-y-2 text-[15px] text-muted-foreground">
              <Point>Apple silicon Macs, macOS 14 Sonoma or later</Point>
              <Point>Signed with a Developer ID and notarized by Apple</Point>
              <Point>Asks once for Input Monitoring (the receiver also carries your typing)</Point>
            </ul>
          </CardContent>
          <CardFooter className="mt-auto">
            <Button asChild size="lg" className="rounded-xl">
              <a href={DMG}>Download for Mac</a>
            </Button>
          </CardFooter>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2 text-2xl">
              <Globe className="size-6" /> In Chrome
            </CardTitle>
            <CardDescription>Any computer, nothing to install.</CardDescription>
          </CardHeader>
          <CardContent>
            <ul className="space-y-2 text-[15px] text-muted-foreground">
              <Point>Chrome, Edge or Opera on Mac, Windows, Linux or ChromeOS (WebHID)</Point>
              <Point>Nothing to install; your macros stay in this browser</Point>
              <Point>Not Safari or Firefox: they can't talk to USB devices</Point>
            </ul>
          </CardContent>
          <CardFooter className="mt-auto">
            <Button asChild size="lg" variant="outline" className="rounded-xl">
              <a href={APP_URL}>Open the web app</a>
            </Button>
          </CardFooter>
        </Card>
      </div>
    </Section>
  )
}

const faq = [
  [
    "Which keyboards does it support?",
    "The AULA F75 (sold as “AULA 75”), through its 2.4 GHz receiver (3554:fa09) or its USB-C cable (258a:010c). Before it writes anything, it checks that the key layout looks like an F75's.",
  ],
  [
    "Does it work over Bluetooth?",
    "No: the keyboard only accepts settings over the receiver or the cable. Settings you write keep working over Bluetooth.",
  ],
  [
    "Why does macOS ask for Input Monitoring?",
    "The receiver carries your keystrokes on the same USB device as its settings, so macOS asks before any app can open it. The app only reads the keyboard's settings channel.",
  ],
  [
    "Can I undo everything?",
    "Yes. “Restore factory keys” puts the base layer back as it shipped, and the Fn layer goes back to the copy the app saved the first time it read it.",
  ],
  [
    "Chrome says no compatible devices?",
    "Set the keyboard's switch to 2.4G and press a key to wake it, then choose Connect keyboard again. On Linux, install the udev rule from the project so Chrome may open the receiver.",
  ],
]

function Faq() {
  return (
    <Section id="faq" title="Questions">
      <Accordion type="single" collapsible className="max-w-3xl">
        {faq.map(([q, a]) => (
          <AccordionItem key={q} value={q}>
            <AccordionTrigger className="text-base">{q}</AccordionTrigger>
            <AccordionContent className="text-[15px] text-muted-foreground">{a}</AccordionContent>
          </AccordionItem>
        ))}
      </Accordion>
    </Section>
  )
}

function Footer() {
  return (
    <footer className="mx-auto mt-28 max-w-6xl px-6 pb-12">
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-4 pt-6 text-sm text-muted-foreground">
        <span className="flex items-center gap-2">
          <Logo /> AULA 75 Control is an independent project, not made by or affiliated with AULA.
        </span>
        <Button variant="ghost" size="sm" asChild>
          <a href="#">
            Back to top <ArrowUp className="size-4" />
          </a>
        </Button>
      </div>
    </footer>
  )
}

export default function App() {
  return (
    <>
      <Nav />
      <main>
        <Hero />
        <HowItWorks />
        <Features />
        <Picker />
        <GetIt />
        <Faq />
      </main>
      <Footer />
    </>
  )
}
