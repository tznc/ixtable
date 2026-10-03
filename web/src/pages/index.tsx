import type { ReactNode } from "react";
import Link from "@docusaurus/Link";
import useBrokenLinks from "@docusaurus/useBrokenLinks";
import Layout from "@theme/Layout";
import WaitlistForm from "@site/src/components/WaitlistForm";
import styles from "./index.module.css";
import dataViewImage from "../../docs/assets/data-view.png";
import formBuilderImage from "../../docs/assets/design-view.png";
import appPreviewImage from "../../docs/assets/app-preview.png";
// Self-hosted copy of an Unsplash photo, so the landing page makes no third-party requests.
import workspacePhoto from "@site/static/img/workspace.jpg";

const features = [
  {
    number: "01",
    title: "Model your business",
    text: "Turn customers, inventory, projects, and processes into structured relational data.",
  },
  {
    number: "02",
    title: "Build the app you need",
    text: "Create focused views and workflows around your data without starting from code.",
  },
  {
    number: "03",
    title: "Make it useful for everyone",
    text: "Give your team a purpose-built tool instead of another fragile spreadsheet.",
  },
];

function ProductPreview(): ReactNode {
  return (
    <div className={styles.previewShell} aria-label="Preview of the ixtable app builder">
      <div className={styles.previewTopbar}>
        <span className={styles.previewLogo}>ix</span>
        <span className={styles.previewBrand}>ixtable</span>
        <span className={styles.previewSearch}>⌕&nbsp;&nbsp; Search records</span>
        <span className={styles.avatar}>MC</span>
      </div>
      <div className={styles.previewBody}>
        <aside className={styles.previewSidebar}>
          <span className={styles.sideActive}>▦&nbsp;&nbsp; Tables</span>
          <span>◫&nbsp;&nbsp; Forms</span>
          <span>⚙&nbsp;&nbsp; Settings</span>
        </aside>
        <div className={styles.previewContent}>
          <div className={styles.previewHeading}>
            <div>
              <small>INVENTORY APP / TABLES</small>
              <h3>Products</h3>
            </div>
            <button type="button">＋ New record</button>
          </div>
          <div className={styles.tableHead}>
            <span>PROJECT</span>
            <span>OWNER</span>
            <span>STATUS</span>
            <span>UPDATED</span>
          </div>
          {[
            ["Studio chair", "Maya Chen", "Active", "Today"],
            ["Task lamp", "Noah Williams", "Review", "Yesterday"],
            ["Walnut desk", "Ava Patel", "Draft", "Aug 12"],
            ["Wool organizer", "Liam Scott", "Active", "Aug 10"],
          ].map((row) => (
            <div className={styles.tableRow} key={row[0]}>
              <span>
                <b>{row[0]}</b>
                <small>Shared workspace</small>
              </span>
              <span>{row[1]}</span>
              <span>
                <i data-status={row[2]} />
                {row[2]}
              </span>
              <span>{row[3]}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function Home(): ReactNode {
  // The waitlist anchor is a plain div, so register it for the broken-anchor check.
  useBrokenLinks().collectAnchor("waitlist");
  return (
    <Layout
      title="Build the business app you need"
      description="ixtable is a free, open-source desktop app builder for relational business apps. Join the waitlist for ixtable Cloud private distribution."
    >
      <main className={styles.page}>
        <section className={styles.hero}>
          <div className={styles.eyebrow}>THE MODERN WAY TO BUILD DATABASE APPS</div>
          <h1>
            Build the app
            <br />
            <em>your business needs.</em>
          </h1>
          <p>
            ixtable is a free, open-source desktop app builder, a modern take on Microsoft Access.
            Build relational business apps locally. ixtable Cloud, the paid service for private
            distribution, opens to the public soon.
          </p>
          <div className={styles.waitlist} id="waitlist">
            <WaitlistForm />
          </div>
          <div className={styles.actions}>
            <Link className={styles.textButton} href="https://github.com/ixtable/ixtable">
              View on GitHub <span>↗</span>
            </Link>
            <button type="button" className={styles.disabledButton} disabled aria-disabled="true">
              Download: signed installers coming soon
            </button>
          </div>
          <p className={styles.microcopy}>
            Free under Apache-2.0 · Windows, macOS, and Linux · Your data stays local
          </p>
          <ProductPreview />
        </section>

        <section className={styles.statement}>
          <p className={styles.kicker}>FROM RAW DATA TO A REAL APPLICATION</p>
          <h2>
            More capable than a spreadsheet.
            <br />
            More approachable <em>than code.</em>
          </h2>
          <div className={styles.featureList}>
            {features.map((feature, index) => (
              <article className={styles.featureRow} key={feature.number}>
                <div className={styles.featureCopy}>
                  <span>{feature.number}</span>
                  <h3>{feature.title}</h3>
                  <p>{feature.text}</p>
                </div>
                <div className={styles.featureVisual}>
                  <img
                    src={
                      index === 0 ? dataViewImage : index === 1 ? formBuilderImage : appPreviewImage
                    }
                    alt={
                      index === 0
                        ? "ixtable DataView TableView showing database records"
                        : index === 1
                          ? "ixtable production form builder"
                          : "Published inventory app built with ixtable"
                    }
                  />
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className={styles.splitSection}>
          <div className={styles.photoWrap}>
            <img
              src={workspacePhoto}
              alt="A tidy workspace with a notebook, keyboard, and monitor"
              loading="eager"
            />
            <span className={styles.photoNote}>
              Your process deserves
              <br />
              an app that fits.
            </span>
          </div>
          <div className={styles.splitCopy}>
            <span className={styles.sectionNumber}>/ 01</span>
            <p className={styles.kicker}>MODEL</p>
            <h2>Give your business data a proper home.</h2>
            <p>
              Move beyond disconnected sheets. Define the tables and relationships behind your
              operation, then keep every record consistent, searchable, and ready to use.
            </p>
            <Link to="/docs/concepts/data-view">
              Explore the data model <span>→</span>
            </Link>
          </div>
        </section>

        <section className={styles.workflowSection}>
          <div className={styles.workflowCopy}>
            <span className={styles.sectionNumber}>/ 02</span>
            <p className={styles.kicker}>BUILD</p>
            <h2>
              Your process.
              <br />
              Your application.
            </h2>
            <p>
              Shape your data into a focused tool for the job. Create the views your team needs and
              leave the one-size-fits-all software behind.
            </p>
          </div>
          <div className={styles.statusCard}>
            <div className={styles.statusHeader}>
              <span>Inventory app</span>
              <b>12 tables</b>
            </div>
            <div className={styles.statusBars}>
              <i />
              <i />
              <i />
            </div>
            <div className={styles.statusLegend}>
              <span>
                <b>7</b> Active
              </span>
              <span>
                <b>3</b> Review
              </span>
              <span>
                <b>2</b> Draft
              </span>
            </div>
            <div className={styles.saved}>
              <span>✓</span>
              <div>
                <b>App ready</b>
                <small>Your latest schema is saved</small>
              </div>
            </div>
          </div>
        </section>

        <section className={styles.cta}>
          <p className={styles.kicker}>BUILD BEYOND THE SPREADSHEET</p>
          <h2>
            Your next business app
            <br />
            <em>starts with a table.</em>
          </h2>
          <p>Build a modern database app around the way your team actually works.</p>
          <div className={styles.ctaLinks}>
            <Link className={styles.primaryButton} href="#waitlist">
              Join the waitlist <span>↑</span>
            </Link>
            <Link className={styles.textButton} to="/pricing">
              See pricing <span>→</span>
            </Link>
          </div>
        </section>
      </main>
    </Layout>
  );
}
