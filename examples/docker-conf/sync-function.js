
function sync(doc, oldDoc, meta) {
    console.log("=== New document revision ===");
    console.log("New doc: ", doc);
    console.log("Old doc: ", oldDoc);
    console.log("Metadata: ", meta);

    if(doc.channels) {
        channel(doc.channels);
    }
    if(doc.expiry) {
        // Format: "2022-06-23T05:00:00+01:00"
        expiry(doc.expiry);
    }

    console.log("=== Document processed ===");
}
