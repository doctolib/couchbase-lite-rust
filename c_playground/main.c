#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include <cbl/CBLCollection.h>
#include <cbl/CBLDatabase.h>
#include <cbl/CBLDocument.h>
#include <cbl/CBLLog.h>
#include <cbl/CBLQuery.h>
#include <cbl/CBLReplicator.h>

const char* DOMAINS[] = { "Database", "Query", "Replicator", "Network" };
const char* LEVEL_PREFIX[] = { "((", "_", "", "WARNING: ", "***ERROR: " };
const char* LEVEL_SUFFIX[] = { "))", "_", "", "", " ***" };

void log_callback(CBLLogDomain domain, CBLLogLevel level, FLString message) {
    printf(
        "CBL %s: %s%s%s\n",
        DOMAINS[domain],
        LEVEL_PREFIX[level],
        (char*) message.buf,
        LEVEL_SUFFIX[level]
    );
}

void startReplication(CBLDatabase *db, bool writer, bool deleter) {
    CBLError error;
    CBLEndpoint* endpoint = CBLEndpoint_CreateWithURL(FLSTR("wss://sync-gateway-staging.doctolib.com:443/billeo-db"), &error);
    assert(endpoint);

    char* token = writer ? "0febaaafc5368d7e2f8663e0ee08b024a47278c1"
        : (deleter ? "61b8b461214c7d6c6c7365dbc4e824111bc4167a"
            : "49230c1a31db39e1d5e96e5fbdf1bf93099b53b5"
        );
    char cookie[64];
    snprintf(cookie, sizeof cookie, "SyncGatewaySession=%s", token);

    FLMutableDict headers = FLMutableDict_New();
    FLMutableDict_SetString(headers, FLSTR("Cookie"), FLStr(cookie));

    FLMutableArray emptyArray = FLMutableArray_New();
    FLArray array = FLMutableArray_GetSource(emptyArray);

    CBLReplicatorConfiguration config = {
        .database = db,
        .endpoint = endpoint,
        .replicatorType = kCBLReplicatorTypePushAndPull,
        .continuous = true,
        .disableAutoPurge = true,
        .maxAttempts = 1,
        .maxAttemptWaitTime = 0,
        .heartbeat = 55,
        .authenticator = NULL,
        .proxy = NULL,
        .headers = headers,
        .pinnedServerCertificate = FLStr(NULL),
        .trustedRootCertificates = FLStr(NULL),
        .channels = array,
        .documentIDs = array,
        .pushFilter = NULL,
        .pullFilter = NULL,
        .conflictResolver = NULL,
        .context = NULL,
        .collections = NULL,
        .collectionCount = 0,
        .acceptParentDomainCookies = false,
    };

    CBLReplicator* replicator = CBLReplicator_Create(&config, &error);
    assert(replicator);

    CBLReplicator_Start(replicator, false);
}

void createDocuments(CBLDatabase *db) {
    CBLError error;

    FILE* fp = fopen("replication_issue.json", "r");
    if (!fp) {
        printf("Failed to open replication_issue.json\n");
        return;
    }

    fseek(fp, 0, SEEK_END);
    long file_size = ftell(fp);
    fseek(fp, 0, SEEK_SET);

    char* json_format = malloc(file_size + 1);
    if (!json_format) {
        printf("Failed to allocate memory for JSON\n");
        fclose(fp);
        return;
    }

    size_t len = fread(json_format, 1, file_size, fp);
    json_format[len] = '\0';
    
    fclose(fp);

    FLString json = {};
    json.buf = json_format;
    json.size = len;

    // Save the document
    const int idOffset = 100;
    for (int i = 0; i < 100; ++i) {
        char id[64];
        snprintf(id, sizeof id, "replication_issue_%d", idOffset + i);
        
        CBLDocument* doc = CBLDocument_CreateWithID(FLStr(id));
        bool set_doc_content = CBLDocument_SetJSON(doc, json, &error);
        assert(set_doc_content);

        FLMutableDict properties = CBLDocument_MutableProperties(doc);
        FLMutableDict_SetString(properties, FLSTR("owner"), FLSTR("00102204"));

        bool saved = CBLDatabase_SaveDocument(db, doc, &error);
        assert(saved);
        
        CBLDocument_Release(doc);
    }

    free(json_format);
}

void getRemainingDocuments(CBLDatabase *db, FLString* result, int* count) {
    CBLError error;
    int errorPos = 0;
    CBLQuery* query = CBLDatabase_CreateQuery(
        db,
        kCBLN1QLLanguage,
        FLSTR("SELECT meta().id FROM _ WHERE _.type='ReplicationIssue' LIMIT 10"),
        &errorPos,
        &error
    );
    assert(query);

    CBLResultSet* queryResult = CBLQuery_Execute(query, &error);
    assert(queryResult);

    int i = 0;
    while (CBLResultSet_Next(queryResult) && i < 10) {
        FLValue id = CBLResultSet_ValueAtIndex(queryResult, 0);
        result[i++] = FLSliceResult_AsSlice(FLSlice_Copy(FLValue_AsString(id)));
    }
    *count = i;

    CBLResultSet_Release(queryResult);
    CBLQuery_Release(query);
}

int getDocumentCount(CBLDatabase *db) {
    CBLError error;
    int errorPos = 0;
    CBLQuery* query = CBLDatabase_CreateQuery(
        db,
        kCBLN1QLLanguage,
        FLSTR("SELECT COUNT(*) FROM _ WHERE _.type='ReplicationIssue'"),
        &errorPos,
        &error
    );
    assert(query);

    CBLResultSet* queryResult = CBLQuery_Execute(query, &error);
    assert(queryResult);

    assert(CBLResultSet_Next(queryResult));
    FLValue count = CBLResultSet_ValueAtIndex(queryResult, 0);

    int result = FLValue_AsInt(count);

    CBLResultSet_Release(queryResult);
    CBLQuery_Release(query);

    return result;
}

void deleteDocuments(CBLDatabase *db) {
    bool remaining = true;
    CBLError error;

    while (remaining) {
        sleep(1);

        FLString documents[10];
        int count = 0;
        getRemainingDocuments(db, documents, &count);

        remaining = (count > 0);

        for (int i = 0; i < count; i++) {
            const CBLDocument* doc = CBLDatabase_GetDocument(db, documents[i], &error);
            if (doc) {
                bool deleted = CBLDatabase_DeleteDocument(db, doc, &error);
                assert(deleted);
                CBLDocument_Release(doc);
            }
        }
    }
}

int main(void) {
    CBLConsoleLogSink log_sink = {};
    log_sink.level = kCBLLogDebug;
    log_sink.domains = kCBLLogDomainMaskAll;

    CBLLogSinks_SetConsole(log_sink);

    // Step configuration
    bool writer = false;
    bool deleter = false;

    FLSlice databaseName = writer || deleter ? FLSTR("writer") : FLSTR("observer");

    // Open database
    CBLError error;
    CBLDatabaseConfiguration config = {FLSTR("/Users/antoinemenciere/Documents")};
    CBLDatabase* db = CBLDatabase_Open(databaseName, &config, &error);
    assert(db);

    // Start a replication
    startReplication(db, writer, deleter);

    // Create 100 documents if the 'writer' variable is on
    if (writer) {
        printf("\nStart creating documents\n\n");
        createDocuments(db);
        printf("\nFinish creating documents\n\n");
    }

    // Delete all documents if the 'deleter' variable is on
    if (deleter) {
        printf("\nStart deleting documents\n\n");
        deleteDocuments(db);
        printf("\nFinish deleting documents\n\n");
    }

    // Always end by an infinite loop, to let the replication run as long as needed
    while (true) {
        sleep(1);

        int count = getDocumentCount(db);
        printf("\nThere is %d document(s) in database\n\n", count);
    }
}
