function sync(doc, oldDoc, meta) {
    console.log("=== New document revision ===");
    console.log("New doc:");
    console.log(doc);
    console.log("Old doc:");
    console.log(oldDoc);
    console.log("Metadata:");
    console.log(meta);

    // Test logic for BC-994: Handle resurrection after tombstone purge
    // Detect documents resurrecting without oldDoc after tombstone expiry
    if (!oldDoc && doc.updatedAt) {
        var ONE_HOUR_MS = 60 * 60 * 1000;
        var updatedAtTimestamp = new Date(doc.updatedAt).getTime();
        var cutoffTimestamp = Date.now() - ONE_HOUR_MS;

        if (updatedAtTimestamp < cutoffTimestamp) {
            // Document is resurrecting after tombstone expired
            // Route to soft_deleted channel so auto-purge will remove from cblite
            console.log(">>> Soft deleting document: updatedAt is older than 1 hour");
            channel("soft_deleted");
            // Set TTL to 5 minutes for testing (production would use 6 months)
            expiry(5 * 60); // 5 minutes in seconds
            console.log(">>> Document routed to soft_deleted channel with 5-minute TTL");
            return;
        }
    }

    if(doc.channels) {
        channel(doc.channels);
    }
    if(doc.expiry) {
        // Format: "2022-06-23T05:00:00+01:00"
        expiry(doc.expiry);
    }

    console.log("=== Document processed ===");
}
