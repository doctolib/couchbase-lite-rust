#include <assert.h>
#include <stdio.h>
#include <string.h>

#include <cbl/CBLCollection.h>
#include <cbl/CBLDatabase.h>
#include <cbl/CBLDocument.h>
#include <cbl/CBLLog.h>
#include <cbl/CBLQuery.h>

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

int main(void) {
    CBLLog_SetCallbackLevel(kCBLLogVerbose);
    CBLLog_SetConsoleLevel(kCBLLogVerbose);
    CBLLog_SetCallback(log_callback);

    // Open database
    CBLError error;
    CBLDatabaseConfiguration config = {FLSTR("/tmp")};
    CBLDatabase* db = CBLDatabase_Open(FLSTR("my_db"), &config, &error);
    assert(db);

    CBLCollection* default_collection = CBLDatabase_DefaultCollection(db, &error);
    assert(default_collection);

    // Create a document
    CBLDocument* doc = CBLDocument_Create();

    FILE* fp = fopen("doc.json", "r");

    char json_format[4096];
    int len = fread(json_format, 1, 4096, fp);
    
    fclose(fp);

    FLString json = {};
    json.buf = &json_format;
    json.size = len;
    bool set_doc_content = CBLDocument_SetJSON(doc, json, &error);

    // Save the document
    bool saved = CBLDatabase_SaveDocument(db, doc, &error);
    assert(saved);

    CBLDocument_Release(doc);

    // Simple array index
    CBLArrayIndexConfiguration array_index_config = {};
    array_index_config.expressionLanguage = kCBLN1QLLanguage;
    array_index_config.path = FLSTR("likes");
    array_index_config.expressions = FLSTR("");

    bool array_index_created = CBLCollection_CreateArrayIndex(
        default_collection,
        FLSTR("one_level"),
        array_index_config,
        &error
    );
    assert(array_index_created);

    int error_pos = 0;
    CBLQuery* query = CBLDatabase_CreateQuery(
        db,
        kCBLN1QLLanguage,
        FLSTR("SELECT _.name, _like FROM _ UNNEST _.likes as _like WHERE _like = 'travel'"),
        &error_pos,
        &error
    );
    assert(query);

    FLSliceResult explain_result = CBLQuery_Explain(query);
    assert(strstr(explain_result.buf, "USING INDEX one_level"));

    CBLResultSet* query_result = CBLQuery_Execute(query, &error);
    assert(query_result);

    assert(CBLResultSet_Next(query_result));

    FLArray row = CBLResultSet_ResultArray(query_result);
    FLValue name = FLArray_Get(row, 0);
    assert(strcmp(FLValue_AsString(name).buf, "Sam") == 0);

    assert(!CBLResultSet_Next(query_result));

    CBLResultSet_Release(query_result);
    CBLQuery_Release(query);

    // Complex array index
    array_index_config.expressionLanguage = kCBLN1QLLanguage;
    array_index_config.path = FLSTR("contacts[].phones");
    array_index_config.expressions = FLSTR("type");

    array_index_created = CBLCollection_CreateArrayIndex(
        default_collection,
        FLSTR("two_level"),
        array_index_config,
        &error
    );
    assert(array_index_created);

    query = CBLDatabase_CreateQuery(
        db,
        kCBLN1QLLanguage,
        FLSTR("SELECT _.name, contact.type, phone.number FROM _ UNNEST _.contacts as contact UNNEST contact.phones as phone WHERE phone.type = 'mobile'"),
        &error_pos,
        &error
    );
    assert(query);

    explain_result = CBLQuery_Explain(query);
    assert(strstr(explain_result.buf, "USING INDEX two_level"));

    query_result = CBLQuery_Execute(query, &error);
    assert(query_result);

    assert(CBLResultSet_Next(query_result));

    row = CBLResultSet_ResultArray(query_result);
    name = FLArray_Get(row, 0);
    assert(strcmp(FLValue_AsString(name).buf, "Sam") == 0);

    assert(CBLResultSet_Next(query_result));
    assert(!CBLResultSet_Next(query_result));

    CBLResultSet_Release(query_result);
    CBLQuery_Release(query);

    // Cleanup
    bool closed = CBLDatabase_Delete(db, &error);
    assert(closed);
}
